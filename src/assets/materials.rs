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
    /// `$basetexturetransform` as UV rows, see [`Materials::xform`].
    pub xform: Option<[f32; 6]>,
}

/// The parts of a VMT that matter for drawing.
#[derive(Default)]
struct VmtInfo {
    base_texture: Option<String>,
    /// 0 = opaque, 1 = alpha test ($alphatest), 2 = blended ($translucent)
    alpha: u8,
    xform: Option<[f32; 6]>,
}

/// Parses `"center .5 .5 scale 2 2 rotate 45 translate 0 0"` (any part optional, or a raw
/// 2x4 matrix) into UV rows. Order as in Source: scale and rotate about the center, then translate.
fn parse_uv_transform(text: &str) -> Option<[f32; 6]> {
    let t = text.trim().trim_matches('"');
    let words: Vec<&str> = t.split_whitespace().collect();
    let num = |w: &str| w.trim_matches(|c| c == '[' || c == ']' || c == '"').parse::<f32>().ok();
    let (mut center, mut scale, mut rot, mut trans) = ([0.5f32, 0.5], [1.0f32, 1.0], 0.0f32, [0.0f32, 0.0]);
    let mut i = 0;
    let mut any = false;
    while i < words.len() {
        let n = |k: usize| words.get(i + k).and_then(|w| num(w));
        match words[i].to_ascii_lowercase().as_str() {
            "center" => { center = [n(1)?, n(2)?]; i += 3; any = true; }
            "scale" => { scale = [n(1)?, n(2)?]; i += 3; any = true; }
            "rotate" => { rot = n(1)?; i += 2; any = true; }
            "translate" => { trans = [n(1)?, n(2)?]; i += 3; any = true; }
            _ => i += 1,
        }
    }
    if !any {
        return None;
    }
    let (sn, cs) = rot.to_radians().sin_cos();
    // p' = R * S * (p - center) + center + translate
    let (a, b, d, e) = (cs * scale[0], -sn * scale[1], sn * scale[0], cs * scale[1]);
    let c = center[0] + trans[0] - (a * center[0] + b * center[1]);
    let f = center[1] + trans[1] - (d * center[0] + e * center[1]);
    let ident = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    let m = [a, b, c, d, e, f];
    (m != ident).then_some(m)
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

    /// What the renderer needs from a VMT, following `include` (patch shaders). Values set in the
    /// material itself win over the included one.
    fn vmt(&self, mat: &str, depth: u32) -> VmtInfo {
        let mut info = VmtInfo::default();
        if depth > 4 {
            return info;
        }
        let Some(bytes) = self.fs.read(&format!("materials/{}.vmt", mat.to_ascii_lowercase())) else { return info };
        let Ok(nodes) = kv::parse(&String::from_utf8_lossy(&bytes)) else { return info };
        let on = |k: &str| find_key(&nodes, k).map_or(false, |v| v.trim().trim_matches('"') != "0" && !v.trim().is_empty());
        info.base_texture = find_key(&nodes, "$basetexture");
        info.xform = find_key(&nodes, "$basetexturetransform").and_then(|t| parse_uv_transform(&t));
        // sprite shaders blend by the texture alpha; otherwise texture alpha is only a mask
        // (specular, self-illum, ...) and must be ignored
        if nodes.first().map_or(false, |n| n.key.to_ascii_lowercase().starts_with("sprite")) || on("$translucent") {
            info.alpha = 2;
        } else if on("$alphatest") {
            info.alpha = 1;
        }
        if let Some(inc) = find_key(&nodes, "include") {
            let inc = inc.trim_start_matches("materials/").trim_end_matches(".vmt").to_string();
            let base = self.vmt(&inc, depth + 1);
            info.base_texture = info.base_texture.or(base.base_texture);
            info.xform = info.xform.or(base.xform);
            info.alpha = info.alpha.max(base.alpha);
        }
        info
    }

    /// The `$basetexturetransform` of a loaded material, as UV rows `[a, b, c, d, e, f]`
    /// (`u' = a*u + b*v + c`, `v' = d*u + e*v + f`).
    pub fn xform(&self, mat: &str) -> Option<[f32; 6]> {
        self.info.get(&mat.to_ascii_lowercase().replace('\\', "/")).and_then(|i| i.xform)
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
        let mut info = MatInfo { w: 64, h: 64, avg: [0.6, 0.6, 0.6], has_texture: false, xform: None };
        let vmt = self.vmt(&key, 0);
        let mode = vmt.alpha;
        info.xform = vmt.xform;
        if let Some(tex) = vmt.base_texture {
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
                        xform: vmt.xform,
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
        if let Some(tex) = self.vmt(&key, 0).base_texture {
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

#[cfg(test)]
mod tests {
    use super::parse_uv_transform;

    #[test]
    fn scale_about_center() {
        let m = parse_uv_transform("\"center .5 .5 scale 2 2 rotate 0 translate 0 0\"").unwrap();
        // the center stays put, a point 0.25 right of it moves 0.5
        let at = |u: f32, v: f32| (m[0] * u + m[1] * v + m[2], m[3] * u + m[4] * v + m[5]);
        let c = at(0.5, 0.5);
        assert!((c.0 - 0.5).abs() < 1e-5 && (c.1 - 0.5).abs() < 1e-5);
        assert!((at(0.75, 0.5).0 - 1.0).abs() < 1e-5);
    }

    #[test]
    fn translate_and_identity() {
        let m = parse_uv_transform("center .5 .5 scale 1 1 rotate 0 translate 0.25 0").unwrap();
        assert!((m[2] - 0.25).abs() < 1e-5);
        assert!(parse_uv_transform("center .5 .5 scale 1 1 rotate 0 translate 0 0").is_none());
        assert!(parse_uv_transform("garbage").is_none());
    }
}
