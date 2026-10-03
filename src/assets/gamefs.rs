//! Game file system: directories and VPK archives mounted from gameinfo `SearchPaths`.

use crate::assets::searchpaths::{self, Mount};
use crate::assets::vpk::Vpk;
use std::path::{Path, PathBuf};

/// A mounted source of files, searched in order.
enum Source {
    Dir(PathBuf),
    Pak(Vpk),
}

pub struct GameFs {
    sources: Vec<Source>,
}

impl GameFs {
    /// Mounts the game from its `gameinfo.txt` `SearchPaths`. Sibling `<game>_dlcN` folders
    /// (mounted implicitly by the engine) take priority; without a usable gameinfo only the
    /// game folder itself is mounted.
    pub fn new(game_dir: &Path) -> GameFs {
        let mut mounts: Vec<Mount> = dlc_dirs(game_dir).into_iter().map(Mount::Dir).collect();
        for m in searchpaths::load(game_dir) {
            if !mounts.contains(&m) {
                mounts.push(m);
            }
        }
        let own = Mount::Dir(game_dir.to_path_buf());
        if !mounts.contains(&own) {
            mounts.push(own);
        }
        let mut sources = Vec::new();
        for m in mounts {
            match m {
                Mount::Dir(d) => {
                    sources.push(Source::Dir(d.clone()));
                    // The engine mounts `pak01_dir.vpk` next to every search directory.
                    let pak = d.join("pak01_dir.vpk");
                    if pak.is_file() && !sources.iter().any(|s| matches!(s, Source::Pak(v) if v.dir_path() == pak)) {
                        sources.extend(Vpk::open(&pak).map(Source::Pak));
                    }
                }
                Mount::Vpk(p) => {
                    if !sources.iter().any(|s| matches!(s, Source::Pak(v) if v.dir_path() == p)) {
                        sources.extend(Vpk::open(&p).map(Source::Pak));
                    }
                }
            }
        }
        GameFs { sources }
    }

    pub fn read(&self, rel: &str) -> Option<Vec<u8>> {
        let rel = rel.replace('\\', "/");
        self.sources.iter().find_map(|s| match s {
            Source::Dir(d) => std::fs::read(d.join(&rel)).ok(),
            Source::Pak(v) => v.read(&rel),
        })
    }

    pub fn exists(&self, rel: &str) -> bool {
        let rel = rel.replace('\\', "/");
        self.sources.iter().any(|s| match s {
            Source::Dir(d) => d.join(&rel).exists(),
            Source::Pak(v) => v.files.contains_key(&rel.to_ascii_lowercase()),
        })
    }

    /// List files under `prefix/` (lowercase, forward slashes) with the given extension.
    pub fn list(&self, prefix: &str, ext: &str) -> Vec<String> {
        let mut out = std::collections::BTreeSet::new();
        let suffix = format!(".{ext}");
        for s in &self.sources {
            match s {
                Source::Pak(v) => {
                    for k in v.files.keys() {
                        if k.starts_with(prefix) && k.ends_with(&suffix) {
                            out.insert(k.clone());
                        }
                    }
                }
                Source::Dir(d) => walk(&d.join(prefix), d, &suffix, &mut out),
            }
        }
        out.into_iter().collect()
    }
}

/// `<game>_dlc*` siblings of the game folder (without language packs), highest DLC first.
fn dlc_dirs(game_dir: &Path) -> Vec<PathBuf> {
    let (Some(parent), Some(name)) = (game_dir.parent(), game_dir.file_name()) else { return vec![] };
    let prefix = format!("{}_dlc", name.to_string_lossy());
    let langs = ["_french", "_german", "_russian", "_spanish"];
    let Ok(rd) = std::fs::read_dir(parent) else { return vec![] };
    let mut dlcs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    n.starts_with(&prefix) && !langs.iter().any(|l| n.contains(l))
                })
        })
        .collect();
    dlcs.sort();
    dlcs.reverse(); // higher DLC overrides
    dlcs
}

fn walk(dir: &Path, root: &Path, suffix: &str, out: &mut std::collections::BTreeSet<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, root, suffix, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            let s = rel.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
            if s.ends_with(suffix) {
                out.insert(s);
            }
        }
    }
}
