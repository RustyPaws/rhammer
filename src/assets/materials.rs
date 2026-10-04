//! Material (VMT/VTF) cache feeding the 3D view and the texture browser.

use crate::assets::gamefs::{GameFs, VpkCache};
use crate::formats::vtf;
use crate::kv::{self as kv, Node, Value};
use std::collections::HashMap;
use crate::platform::SharedVfs;
use std::path::Path;

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
    /// Decoded RGBA images awaiting upload to the GPU: (material, w, h, rgba, alpha mode).
    /// Alpha mode: 0 = opaque, 1 = alpha test ($alphatest), 2 = blended ($translucent).
    pub ready: Vec<(String, u32, u32, Vec<u8>, u8)>,
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
    pub fn new(vfs: SharedVfs, vpks: &VpkCache, game_dir: &Path, steam_dir: Option<&Path>) -> Materials {
        let fs = GameFs::new(vfs, vpks, game_dir, steam_dir);
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

    /// Transparency declared by the VMT: 0 = opaque, 1 = alpha test, 2 = translucent.
    /// Texture alpha is otherwise a mask (specular, self-illum, ...) and must be ignored.
    fn alpha_mode(&self, mat: &str, depth: u32) -> u8 {
        if depth > 4 {
            return 0;
        }
        let Some(bytes) = self.fs.read(&format!("materials/{}.vmt", mat.to_ascii_lowercase())) else { return 0 };
        let Ok(nodes) = kv::parse(&String::from_utf8_lossy(&bytes)) else { return 0 };
        let on = |k: &str| find_key(&nodes, k).map_or(false, |v| v.trim().trim_matches('"') != "0" && !v.trim().is_empty());
        if on("$translucent") {
            return 2;
        }
        if on("$alphatest") {
            return 1;
        }
        if let Some(inc) = find_key(&nodes, "include") {
            let inc = inc.trim_start_matches("materials/").trim_end_matches(".vmt").to_string();
            return self.alpha_mode(&inc, depth + 1);
        }
        0
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
        let mark = self.fs.mark();
        let mut info = MatInfo { w: 64, h: 64, avg: [0.6, 0.6, 0.6], has_texture: false };
        let mode = self.alpha_mode(&key, 0);
        if let Some(tex) = self.base_texture(&key, 0) {
            let tex = tex.replace('\\', "/").to_ascii_lowercase();
            if let Some(bytes) = self.fs.read(&format!("materials/{tex}.vtf")) {
                if let Some(img) = vtf::read_vtf(&bytes, 512) {
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
                    self.ready.push((key.clone(), img.w, img.h, img.rgba, mode));
                }
            }
        }
        if !info.has_texture && self.fs.stalled_since(mark) {
            // the files have not all arrived yet: look again next frame
            self.starved = true;
            return None;
        }
        self.info.insert(key, info.clone());
        if info.has_texture { Some(info) } else { None }
    }

    /// Files are still being fetched, so a missing result may change.
    pub fn loading(&self) -> bool {
        self.fs.pending()
    }

    /// Small RGBA preview for the texture browser. `None` also while its files are loading.
    pub fn thumb(&mut self, mat: &str) -> Option<(u32, u32, Vec<u8>)> {
        let key = mat.to_ascii_lowercase();
        if let Some(t) = self.thumbs.get(&key) {
            return t.clone();
        }
        let mark = self.fs.mark();
        let mut res = None;
        if let Some(tex) = self.base_texture(&key, 0) {
            let tex = tex.replace('\\', "/").to_ascii_lowercase();
            if let Some(bytes) = self.fs.read(&format!("materials/{tex}.vtf")) {
                if let Some(img) = vtf::read_vtf(&bytes, 128) {
                    res = Some((img.w, img.h, img.rgba));
                }
            }
        }
        if res.is_none() && self.fs.stalled_since(mark) {
            return None;
        }
        self.thumbs.insert(key, res.clone());
        res
    }
}
