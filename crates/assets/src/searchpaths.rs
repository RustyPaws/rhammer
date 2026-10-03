//! `gameinfo.txt` -> `FileSystem` -> `SearchPaths`: which directories and VPK archives a
//! game mounts, in priority order (first entry wins).

use rhammer_kv::{self as kv, NodeList, Value};
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
pub fn resolve(entries: &[RawEntry], game_dir: &Path) -> Vec<Mount> {
    let root = game_dir.parent().unwrap_or(game_dir);
    let mut out: Vec<Mount> = Vec::new();
    let mut push = |m: Mount| {
        if !out.contains(&m) {
            out.push(m);
        }
    };
    for e in entries {
        let p = e.path.replace('\\', "/");
        let (base, rest) = if let Some(r) = strip_macro(&p, "|gameinfo_path|") {
            (game_dir, r)
        } else if let Some(r) = strip_macro(&p, "|all_source_engine_paths|") {
            (root, r)
        } else {
            (root, p.as_str())
        };
        let rest = rest.trim_start_matches('/');
        if rest.to_ascii_lowercase().ends_with(".vpk") {
            let stem = rest[..rest.len() - 4].trim_end_matches("_dir");
            let dir_vpk = base.join(format!("{stem}_dir.vpk"));
            if dir_vpk.is_file() {
                push(Mount::Vpk(dir_vpk));
            }
        } else if rest == "*" || rest.ends_with("/*") {
            let parent = base.join(rest.trim_end_matches('*').trim_end_matches('/'));
            let mut subs: Vec<PathBuf> = std::fs::read_dir(&parent)
                .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
                .unwrap_or_default();
            subs.sort();
            for s in subs {
                push(Mount::Dir(s));
            }
        } else {
            let dir = if rest.is_empty() || rest == "." { base.to_path_buf() } else { base.join(rest.trim_end_matches("/.")) };
            if dir.is_dir() {
                push(Mount::Dir(dir));
            }
        }
    }
    out
}

fn strip_macro<'a>(p: &'a str, mac: &str) -> Option<&'a str> {
    (p.len() >= mac.len() && p[..mac.len()].eq_ignore_ascii_case(mac)).then(|| &p[mac.len()..])
}

/// Reads `<game_dir>/gameinfo.txt` and returns its mounts (empty if missing or malformed).
pub fn load(game_dir: &Path) -> Vec<Mount> {
    let Ok(bytes) = std::fs::read(game_dir.join("gameinfo.txt")) else { return vec![] };
    match kv::parse(&String::from_utf8_lossy(&bytes)) {
        Ok(root) => resolve(&parse_entries(&root), game_dir),
        Err(_) => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
