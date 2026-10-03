//! Game file system: directories and VPK archives mounted from gameinfo `SearchPaths`.

use crate::assets::searchpaths::{self, Mount};
use crate::assets::vpk::Vpk;
use crate::platform::{SharedVfs, Vfs};
use std::path::{Path, PathBuf};

/// A mounted source of files, searched in order.
enum Source {
    Dir(PathBuf),
    Pak(Vpk),
}

pub struct GameFs {
    vfs: SharedVfs,
    sources: Vec<Source>,
}

impl GameFs {
    /// Mounts the game from its `gameinfo.txt` `SearchPaths`. Sibling `<game>_dlcN` folders
    /// (mounted implicitly by the engine) take priority; without a usable gameinfo only the
    /// game folder itself is mounted.
    pub fn new(vfs: SharedVfs, game_dir: &Path) -> GameFs {
        let mut mounts: Vec<Mount> = dlc_dirs(&*vfs, game_dir).into_iter().map(Mount::Dir).collect();
        for m in searchpaths::load(&*vfs, game_dir) {
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
                    if vfs.is_file(&pak) && !sources.iter().any(|s| matches!(s, Source::Pak(v) if v.dir_path() == pak)) {
                        sources.extend(Vpk::open(&vfs, &pak).map(Source::Pak));
                    }
                }
                Mount::Vpk(p) => {
                    if !sources.iter().any(|s| matches!(s, Source::Pak(v) if v.dir_path() == p)) {
                        sources.extend(Vpk::open(&vfs, &p).map(Source::Pak));
                    }
                }
            }
        }
        GameFs { vfs, sources }
    }

    /// True while file requests are still in flight (browser build).
    pub fn pending(&self) -> bool {
        self.vfs.pending() > 0
    }

    /// Marker for [`GameFs::stalled_since`].
    pub fn mark(&self) -> usize {
        self.vfs.stalls()
    }

    /// Whether a read since `mark` answered "not loaded yet", so its result is incomplete.
    pub fn stalled_since(&self, mark: usize) -> bool {
        self.vfs.stalls() != mark
    }

    pub fn read(&self, rel: &str) -> Option<Vec<u8>> {
        let rel = rel.replace('\\', "/");
        self.sources.iter().find_map(|s| match s {
            Source::Dir(d) => self.vfs.read(&d.join(&rel)),
            Source::Pak(v) => v.read(&rel),
        })
    }

    pub fn exists(&self, rel: &str) -> bool {
        let rel = rel.replace('\\', "/");
        self.sources.iter().any(|s| match s {
            Source::Dir(d) => self.vfs.exists(&d.join(&rel)),
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
                Source::Dir(d) => walk(&*self.vfs, &d.join(prefix), "", prefix, &suffix, &mut out),
            }
        }
        out.into_iter().collect()
    }
}

/// `<game>_dlc*` siblings of the game folder (without language packs), highest DLC first.
fn dlc_dirs(vfs: &dyn Vfs, game_dir: &Path) -> Vec<PathBuf> {
    let (Some(parent), Some(name)) = (game_dir.parent(), game_dir.file_name()) else { return vec![] };
    let prefix = format!("{}_dlc", name.to_string_lossy());
    let langs = ["_french", "_german", "_russian", "_spanish"];
    let mut dlcs: Vec<PathBuf> = vfs
        .read_dir(parent)
        .into_iter()
        .filter(|e| e.is_dir && e.name.starts_with(&prefix) && !langs.iter().any(|l| e.name.contains(l)))
        .map(|e| parent.join(e.name))
        .collect();
    dlcs.sort();
    dlcs.reverse(); // higher DLC overrides
    dlcs
}

/// Collects files below `dir` whose mount-relative path (`prefix` + `rel`) ends with `suffix`.
fn walk(vfs: &dyn Vfs, dir: &Path, rel: &str, prefix: &str, suffix: &str, out: &mut std::collections::BTreeSet<String>) {
    for e in vfs.read_dir(dir) {
        let name = e.name.to_ascii_lowercase();
        if e.is_dir {
            walk(vfs, &dir.join(&e.name), &format!("{rel}{name}/"), prefix, suffix, out);
        } else {
            let s = format!("{prefix}{rel}{name}");
            if s.ends_with(suffix) {
                out.insert(s);
            }
        }
    }
}
