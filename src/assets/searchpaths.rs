//! `gameinfo.txt` -> `FileSystem` -> `SearchPaths`: which directories and VPK archives a
//! game mounts, in priority order (first entry wins).

use crate::assets::steam::SteamLibraries;
use crate::kv::{self as kv, NodeList, Value};
use crate::platform::Vfs;
use std::path::{Path, PathBuf};

/// One `SearchPaths` line before it is resolved against the disk.
#[derive(Clone, Debug, PartialEq)]
pub struct RawEntry {
    /// Lower-cased `+`-separated tags of the key, e.g. `["game", "mod"]`.
    pub tags: Vec<String>,
    pub path: String,
}

/// A resolved mount point.
#[derive(Clone, Debug, PartialEq)]
pub enum Mount {
    /// Directory with loose files (and an optional `pak01_dir.vpk` next to them).
    Dir(PathBuf),
    /// `<name>_dir.vpk` archive.
    Vpk(PathBuf),
}

/// Tags that make an entry readable content (the rest are write-only or executables).
const CONTENT_TAGS: [&str; 4] = ["game", "mod", "platform", "custom_mod"];

/// Extracts the content entries of `SearchPaths` from a parsed gameinfo file.
pub fn parse_entries(root: &[kv::Node]) -> Vec<RawEntry> {
    let Some(info) = root.iter().find(|n| n.key.eq_ignore_ascii_case("GameInfo")).map(|n| n.children()) else {
        return vec![];
    };
    let Some(sp) = info.get_block("FileSystem").and_then(|fs| fs.get_block("SearchPaths")) else {
        return vec![];
    };
    sp.iter()
        .filter_map(|n| {
            let Value::Str(path) = &n.value else { return None };
            let tags: Vec<String> = n.key.split('+').map(|t| t.trim().to_ascii_lowercase()).collect();
            tags.iter().any(|t| CONTENT_TAGS.contains(&t.as_str())).then(|| RawEntry { tags, path: path.trim().to_string() })
        })
        .collect()
}

/// Resolves entries to mounts. `game_dir` is the folder holding `gameinfo.txt`
/// (`|gameinfo_path|`); other relative paths are relative to its parent (the engine root).
/// `|appid_<id>|path` entries (used by the SDK template mods to mount TF2 / HL2 content) are
/// relative to the install folder of that Steam app; they are skipped if it is not installed.
pub fn resolve(vfs: &dyn Vfs, entries: &[RawEntry], game_dir: &Path) -> Vec<Mount> {
    let root = game_dir.parent().unwrap_or(game_dir);
    let mut steam: Option<SteamLibraries> = None;
    let mut out: Vec<Mount> = Vec::new();
    let mut push = |m: Mount| {
        if !out.contains(&m) {
            out.push(m);
        }
    };
    for e in entries {
        let p = e.path.replace('\\', "/");
        let (base, rest) = if let Some(r) = strip_macro(&p, "|gameinfo_path|") {
            (game_dir.to_path_buf(), r)
        } else if let Some(r) = strip_macro(&p, "|all_source_engine_paths|") {
            (root.to_path_buf(), r)
        } else if let Some((id, r)) = split_appid_macro(&p) {
            let libs = steam.get_or_insert_with(|| SteamLibraries::discover(vfs, game_dir));
            let Some(dir) = libs.install_dir(vfs, id) else { continue };
            (dir, r)
        } else {
            (root.to_path_buf(), p.as_str())
        };
        let base = base.as_path();
        let rest = rest.trim_start_matches('/');
        if rest.to_ascii_lowercase().ends_with(".vpk") {
            let stem = rest[..rest.len() - 4].trim_end_matches("_dir");
            let dir_vpk = base.join(format!("{stem}_dir.vpk"));
            if vfs.is_file(&dir_vpk) {
                push(Mount::Vpk(dir_vpk));
            }
        } else if rest == "*" || rest.ends_with("/*") {
            let parent = base.join(rest.trim_end_matches('*').trim_end_matches('/'));
            let mut subs: Vec<PathBuf> = vfs.read_dir(&parent).into_iter().filter(|e| e.is_dir).map(|e| parent.join(e.name)).collect();
            subs.sort();
            for s in subs {
                push(Mount::Dir(s));
            }
        } else {
            let dir = if rest.is_empty() || rest == "." { base.to_path_buf() } else { base.join(rest.trim_end_matches("/.")) };
            if vfs.is_dir(&dir) {
                push(Mount::Dir(dir));
            }
        }
    }
    out
}

fn strip_macro<'a>(p: &'a str, mac: &str) -> Option<&'a str> {
    (p.len() >= mac.len() && p[..mac.len()].eq_ignore_ascii_case(mac)).then(|| &p[mac.len()..])
}

/// Splits `|appid_<id>|rest` into the app id and `rest`.
fn split_appid_macro(p: &str) -> Option<(u32, &str)> {
    const PREFIX: &str = "|appid_";
    if !p.get(..PREFIX.len())?.eq_ignore_ascii_case(PREFIX) {
        return None;
    }
    let (id, rest) = p[PREFIX.len()..].split_once('|')?;
    Some((id.trim().parse().ok()?, rest))
}

/// Reads `<game_dir>/gameinfo.txt` and returns its mounts (empty if missing or malformed).
pub fn load(vfs: &dyn Vfs, game_dir: &Path) -> Vec<Mount> {
    let Some(bytes) = vfs.read(&game_dir.join("gameinfo.txt")) else { return vec![] };
    match kv::parse(&String::from_utf8_lossy(&bytes)) {
        Ok(root) => resolve(vfs, &parse_entries(&root), game_dir),
        Err(_) => vec![],
    }
}

#[cfg(all(test, feature = "local"))]
mod tests {
    use super::*;
    use crate::platform::LocalFs;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rhammer_sp_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_tags_and_skips_write_only() {
        let kv = kv::parse(
            "\"GameInfo\" { FileSystem { SearchPaths {\n game+mod |gameinfo_path|.\n gamebin |gameinfo_path|bin\n default_write_path |gameinfo_path|.\n platform platform\n } } }",
        )
        .unwrap();
        let e = parse_entries(&kv);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].tags, ["game", "mod"]);
        assert_eq!(e[1].path, "platform");
    }

    #[test]
    fn resolves_macros_vpks_and_wildcards() {
        let root = tmp("resolve");
        let game = root.join("mymod");
        fs::create_dir_all(game.join("custom/a")).unwrap();
        fs::create_dir_all(game.join("custom/b")).unwrap();
        fs::create_dir_all(root.join("hl2")).unwrap();
        fs::write(root.join("hl2/hl2_misc_dir.vpk"), b"").unwrap();
        let entry = |p: &str| RawEntry { tags: vec!["game".into()], path: p.into() };
        let m = resolve(
            &LocalFs,
            &[
                entry("|gameinfo_path|custom/*"),
                entry("|gameinfo_path|."),
                entry("hl2/hl2_misc.vpk"),
                entry("hl2"),
                entry("missing"),
                entry("|GAMEINFO_PATH|."),
            ],
            &game,
        );
        assert_eq!(
            m,
            vec![
                Mount::Dir(game.join("custom/a")),
                Mount::Dir(game.join("custom/b")),
                Mount::Dir(game.clone()),
                Mount::Vpk(root.join("hl2/hl2_misc_dir.vpk")),
                Mount::Dir(root.join("hl2")),
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }
}
