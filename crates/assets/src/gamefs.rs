//! Game file system: loose files (game dir + DLC dirs) layered over VPK archives.

use crate::vpk::Vpk;
use std::path::{Path, PathBuf};

pub struct GameFs {
    dirs: Vec<PathBuf>,
    vpks: Vec<Vpk>,
}

impl GameFs {
    pub fn new(game_dir: &Path) -> GameFs {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let (Some(parent), Some(name)) = (game_dir.parent(), game_dir.file_name()) {
            let prefix = format!("{}_dlc", name.to_string_lossy());
            if let Ok(rd) = std::fs::read_dir(parent) {
                let mut dlcs: Vec<PathBuf> = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir() && p.file_name().map(|n| n.to_string_lossy().starts_with(&prefix) && !n.to_string_lossy().contains("_french") && !n.to_string_lossy().contains("_german") && !n.to_string_lossy().contains("_russian") && !n.to_string_lossy().contains("_spanish")).unwrap_or(false))
                    .collect();
                dlcs.sort();
                dlcs.reverse(); // higher DLC overrides
                dirs.extend(dlcs);
            }
        }
        dirs.push(game_dir.to_path_buf());
        let mut vpks = Vec::new();
        for d in &dirs {
            if let Some(v) = Vpk::open(&d.join("pak01_dir.vpk")) {
                vpks.push(v);
            }
        }
        GameFs { dirs, vpks }
    }

    pub fn read(&self, rel: &str) -> Option<Vec<u8>> {
        let rel = rel.replace('\\', "/");
        for d in &self.dirs {
            if let Ok(b) = std::fs::read(d.join(&rel)) {
                return Some(b);
            }
        }
        for v in &self.vpks {
            if let Some(b) = v.read(&rel) {
                return Some(b);
            }
        }
        None
    }

    pub fn exists(&self, rel: &str) -> bool {
        let rel = rel.replace('\\', "/");
        self.dirs.iter().any(|d| d.join(&rel).exists()) || self.vpks.iter().any(|v| v.files.contains_key(&rel.to_ascii_lowercase()))
    }

    /// List files under `prefix/` (lowercase, forward slashes) with the given extension.
    pub fn list(&self, prefix: &str, ext: &str) -> Vec<String> {
        let mut out = std::collections::BTreeSet::new();
        let suffix = format!(".{ext}");
        for v in &self.vpks {
            for k in v.files.keys() {
                if k.starts_with(prefix) && k.ends_with(&suffix) {
                    out.insert(k.clone());
                }
            }
        }
        for d in &self.dirs {
            walk(&d.join(prefix), d, &suffix, &mut out);
        }
        out.into_iter().collect()
    }
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
