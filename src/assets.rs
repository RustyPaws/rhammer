//! Game file system (loose files + VPK) and material cache.

use crate::kv::{self, Node, Value};
use crate::vpk::Vpk;
use std::collections::HashMap;
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

#[derive(Clone, Debug)]
pub struct MatInfo {
    pub w: u32,
    pub h: u32,
    pub avg: [f32; 3],
    pub has_texture: bool,
}

pub struct Materials {
    pub fs: GameFs,
    pub info: HashMap<String, MatInfo>,
    /// Decoded RGBA images awaiting upload to the GPU: (material, w, h, rgba)
    pub ready: Vec<(String, u32, u32, Vec<u8>)>,
    /// For egui previews: material -> RGBA (<=128px)
    pub thumbs: HashMap<String, Option<(u32, u32, Vec<u8>)>>,
    pub all: Vec<String>,
    /// Max number of new textures decoded per frame; `starved` is set when it ran out.
    pub budget: i32,
    pub starved: bool,
}

fn find_key(nodes: &[Node], key: &str) -> Option<String> {
    for n in nodes {
        if n.key.eq_ignore_ascii_case(key) {
            if let Value::Str(v) = &n.value {
                return Some(v.clone());
            }
        }
    }
    for n in nodes {
        if let Value::Block(c) = &n.value {
            if let Some(v) = find_key(c, key) {
                return Some(v);
            }
        }
    }
    None
}

impl Materials {
    pub fn new(game_dir: &Path) -> Materials {
        let fs = GameFs::new(game_dir);
        let all: Vec<String> = fs
            .list("materials/", "vmt")
            .into_iter()
            .map(|s| s.trim_start_matches("materials/").trim_end_matches(".vmt").to_string())
            .collect();
        Materials { fs, info: HashMap::new(), ready: vec![], thumbs: HashMap::new(), all, budget: 12, starved: false }
    }

    fn base_texture(&self, mat: &str, depth: u32) -> Option<String> {
        if depth > 4 {
            return None;
        }
        let bytes = self.fs.read(&format!("materials/{}.vmt", mat.to_ascii_lowercase()))?;
        let nodes = kv::parse(&String::from_utf8_lossy(&bytes)).ok()?;
        if let Some(t) = find_key(&nodes, "$basetexture") {
            return Some(t);
        }
        // patch shader: include another vmt
        if let Some(inc) = find_key(&nodes, "include") {
            let inc = inc.trim_start_matches("materials/").trim_end_matches(".vmt").to_string();
            return self.base_texture(&inc, depth + 1);
        }
        None
    }

    /// Get (loading on first use) material info. Textures are queued for GPU upload.
    pub fn get(&mut self, mat: &str) -> Option<MatInfo> {
        let key = mat.to_ascii_lowercase().replace('\\', "/");
        if let Some(i) = self.info.get(&key) {
            return if i.has_texture { Some(i.clone()) } else { None };
        }
        if self.budget <= 0 {
            self.starved = true;
            return None;
        }
        self.budget -= 1;
        let mut info = MatInfo { w: 64, h: 64, avg: [0.6, 0.6, 0.6], has_texture: false };
        if let Some(tex) = self.base_texture(&key, 0) {
            let tex = tex.replace('\\', "/").to_ascii_lowercase();
            if let Some(bytes) = self.fs.read(&format!("materials/{tex}.vtf")) {
                if let Some(img) = crate::vtf::read_vtf(&bytes, 512) {
                    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
                    for px in img.rgba.chunks_exact(4).step_by(7) {
                        r += px[0] as u64;
                        g += px[1] as u64;
                        b += px[2] as u64;
                        n += 1;
                    }
                    let n = n.max(1) as f32 * 255.0;
                    info = MatInfo {
                        w: img.full_w,
                        h: img.full_h,
                        avg: [r as f32 / n, g as f32 / n, b as f32 / n],
                        has_texture: true,
                    };
                    self.ready.push((key.clone(), img.w, img.h, img.rgba));
                }
            }
        }
        self.info.insert(key, info.clone());
        if info.has_texture { Some(info) } else { None }
    }

    /// Small RGBA preview for the texture browser.
    pub fn thumb(&mut self, mat: &str) -> Option<(u32, u32, Vec<u8>)> {
        let key = mat.to_ascii_lowercase();
        if let Some(t) = self.thumbs.get(&key) {
            return t.clone();
        }
        let mut res = None;
        if let Some(tex) = self.base_texture(&key, 0) {
            let tex = tex.replace('\\', "/").to_ascii_lowercase();
            if let Some(bytes) = self.fs.read(&format!("materials/{tex}.vtf")) {
                if let Some(img) = crate::vtf::read_vtf(&bytes, 128) {
                    res = Some((img.w, img.h, img.rgba));
                }
            }
        }
        self.thumbs.insert(key, res.clone());
        res
    }
}
