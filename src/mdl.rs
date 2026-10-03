//! Source studio model loader (MDL v44-49 + VVD + VTX v7 + ANI). Produces a LOD0 triangle mesh
//! grouped by material, the skeleton and the sequence list, and can pose the mesh at any frame.

use crate::assets::GameFs;
use glam::{DMat4, DQuat, DVec3};

#[derive(Clone, Copy, Default)]
pub struct ModelVert {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub uv: [f32; 2],
}

/// Bind-pose vertex with its bone weights.
#[derive(Clone, Copy, Default)]
pub struct SkinVert {
    pub pos: DVec3,
    pub nrm: DVec3,
    pub uv: [f32; 2],
    pub bones: [u8; 3],
    pub weights: [f32; 3],
    pub nbones: u8,
}

#[derive(Clone, Default)]
pub struct ModelPart {
    /// Candidate material names (one per skin family), e.g. "models/props/metal_box".
    pub materials: Vec<String>,
    /// Triangle list posed at frame 0 of the first sequence.
    pub verts: Vec<ModelVert>,
    /// The same triangle list in the bind pose, for re-skinning.
    pub bind: Vec<SkinVert>,
}

#[derive(Clone, Default)]
pub struct Sequence {
    pub name: String,
    pub activity: String,
    pub fps: f64,
    pub frames: usize,
    pub looping: bool,
    /// Index into `Model::srcs` (0 = the model itself, then `$includemodel`s).
    src: usize,
    /// Animation description index inside that source.
    anim: usize,
    /// Per source-bone weights; bones with weight 0 keep their default pose.
    weights: Vec<f32>,
}

impl Sequence {
    /// Length of one cycle in seconds.
    pub fn duration(&self) -> f64 {
        if self.frames <= 1 || self.fps <= 0.0 {
            0.0
        } else {
            (self.frames - 1) as f64 / self.fps
        }
    }

    /// Frame (fractional) shown at time `t`, looping.
    pub fn frame_at(&self, t: f64) -> f64 {
        let d = self.duration();
        if d <= 0.0 {
            return 0.0;
        }
        (t.rem_euclid(d) * self.fps).min((self.frames - 1) as f64)
    }
}

/// A named attachment point on a bone.
#[derive(Clone, Default)]
pub struct Attachment {
    pub name: String,
    pub bone: usize,
    local: DMat4,
}

#[derive(Clone, Default)]
struct Bone {
    name: String,
    parent: i32,
    pos: DVec3,
    quat: DQuat,
    rot: DVec3,
    posscale: DVec3,
    rotscale: DVec3,
    pose_to_bone: DMat4,
}

/// A file that contributes animations: its MDL bytes, optional .ani, skeleton and bone mapping.
#[derive(Clone, Default)]
struct AnimSrc {
    mdl: Vec<u8>,
    ani: Option<Vec<u8>>,
    bones: Vec<Bone>,
    /// source bone -> model bone
    map: Vec<Option<usize>>,
}

#[derive(Clone, Default)]
pub struct Model {
    pub hull_min: DVec3,
    pub hull_max: DVec3,
    pub parts: Vec<ModelPart>,
    pub sequences: Vec<Sequence>,
    pub attachments: Vec<Attachment>,
    bones: Vec<Bone>,
    srcs: Vec<AnimSrc>,
}

fn i32_at(b: &[u8], o: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn i16_at(b: &[u8], o: usize) -> Option<i16> {
    Some(i16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn f32_at(b: &[u8], o: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn cstr_at(b: &[u8], o: usize) -> String {
    let end = b[o.min(b.len())..].iter().position(|c| *c == 0).map(|p| o + p).unwrap_or(b.len());
    String::from_utf8_lossy(&b[o.min(b.len())..end]).into_owned()
}
fn vec3_at(b: &[u8], o: usize) -> Option<DVec3> {
    Some(DVec3::new(f32_at(b, o)? as f64, f32_at(b, o + 4)? as f64, f32_at(b, o + 8)? as f64))
}
/// String stored at `base + rel`, where `rel` is the int at `at`.
fn rel_str(b: &[u8], base: usize, at: usize) -> String {
    match i32_at(b, at) {
        Some(r) if r != 0 => cstr_at(b, (base as i64 + r as i64).max(0) as usize),
        _ => String::new(),
    }
}

fn f16_to_f64(h: u16) -> f64 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1f) as i32;
    let man = (h & 0x3ff) as f64;
    sign * match exp {
        0 => man * 2f64.powi(-24),
        31 => 0.0, // inf/nan never appear in valid data
        e => (1.0 + man / 1024.0) * 2f64.powi(e - 15),
    }
}

/// Source RadianEuler (roll, pitch, yaw) to quaternion.
fn euler_quat(a: DVec3) -> DQuat {
    DQuat::from_rotation_z(a.z) * DQuat::from_rotation_y(a.y) * DQuat::from_rotation_x(a.x)
}

/// Hull only (cheap): used for sizing entity boxes.
pub fn read_hull(mdl: &[u8]) -> Option<(DVec3, DVec3)> {
    if mdl.len() < 152 || &mdl[0..4] != b"IDST" {
        return None;
    }
    Some((vec3_at(mdl, 104)?, vec3_at(mdl, 116)?))
}

pub fn load(fs: &GameFs, path: &str) -> Option<Model> {
    let path = path.replace('\\', "/").to_ascii_lowercase();
    let mdl = fs.read(&path)?;
    let (hull_min, hull_max) = read_hull(&mdl)?;
    let mut model = Model { hull_min, hull_max, ..Default::default() };
    model.bones = read_bones(&mdl).unwrap_or_default();
    model.attachments = read_attachments(&mdl);

    let stem = path.trim_end_matches(".mdl");
    if let (Some(vvd), Some(vtx)) = (
        fs.read(&format!("{stem}.vvd")),
        fs.read(&format!("{stem}.dx90.vtx")).or_else(|| fs.read(&format!("{stem}.dx80.vtx"))).or_else(|| fs.read(&format!("{stem}.sw.vtx"))),
    ) {
        let _ = build_meshes(&mdl, &vvd, &vtx, fs, &mut model);
    }

    // animation sources: the model itself, then included models (shared animation files)
    let mut files = vec![mdl];
    let first = &files[0];
    let n_inc = i32_at(first, 336).unwrap_or(0).clamp(0, 64) as usize;
    let inc_idx = i32_at(first, 340).unwrap_or(0).max(0) as usize;
    let mut inc_names: Vec<String> = vec![];
    for i in 0..n_inc {
        let g = inc_idx + i * 8;
        let name = rel_str(first, g, g + 4).replace('\\', "/").to_ascii_lowercase();
        if !name.is_empty() && name != path && !inc_names.contains(&name) {
            inc_names.push(name);
        }
    }
    for n in &inc_names {
        if let Some(b) = fs.read(n) {
            if b.len() > 400 && &b[0..4] == b"IDST" {
                files.push(b);
            }
        }
    }
    for (si, data) in files.into_iter().enumerate() {
        let bones = if si == 0 { model.bones.clone() } else { read_bones(&data).unwrap_or_default() };
        let map = bones.iter().map(|b| model.bones.iter().position(|m| m.name.eq_ignore_ascii_case(&b.name))).collect();
        let ani_name = rel_str(&data, 0, 348).replace('\\', "/").to_ascii_lowercase();
        let ani = if i32_at(&data, 352).unwrap_or(0) > 0 && !ani_name.is_empty() {
            fs.read(&ani_name).or_else(|| fs.read(&format!("models/{ani_name}")))
        } else {
            None
        };
        let src = AnimSrc { mdl: data, ani, bones, map };
        let _ = read_sequences(&src, si, &mut model.sequences);
        model.srcs.push(src);
    }

    // default pose: frame 0 of the first sequence
    if !model.sequences.is_empty() {
        if let Some(m) = model.skin_matrices(0, 0.0) {
            for p in &mut model.parts {
                p.verts = p.bind.iter().map(|v| skin_vert(&m, v)).collect();
            }
        }
    }
    Some(model)
}

fn read_bones(mdl: &[u8]) -> Option<Vec<Bone>> {
    let nb = i32_at(mdl, 156)? as usize;
    let bi = i32_at(mdl, 160)? as usize;
    if nb > 256 {
        return None;
    }
    let mut bones = Vec::with_capacity(nb);
    for b in 0..nb {
        let o = bi + b * 216;
        let q = [f32_at(mdl, o + 44)?, f32_at(mdl, o + 48)?, f32_at(mdl, o + 52)?, f32_at(mdl, o + 56)?];
        let mut m = [0f64; 12];
        for (k, v) in m.iter_mut().enumerate() {
            *v = f32_at(mdl, o + 96 + k * 4)? as f64;
        }
        bones.push(Bone {
            name: rel_str(mdl, o, o),
            parent: i32_at(mdl, o + 4)?,
            pos: vec3_at(mdl, o + 32)?,
            quat: DQuat::from_xyzw(q[0] as f64, q[1] as f64, q[2] as f64, q[3] as f64),
            rot: vec3_at(mdl, o + 60)?,
            posscale: vec3_at(mdl, o + 72)?,
            rotscale: vec3_at(mdl, o + 84)?,
            // stored as a row-major 3x4
            pose_to_bone: DMat4::from_cols_array(&[m[0], m[4], m[8], 0.0, m[1], m[5], m[9], 0.0, m[2], m[6], m[10], 0.0, m[3], m[7], m[11], 1.0]),
        });
    }
    Some(bones)
}

fn read_attachments(mdl: &[u8]) -> Vec<Attachment> {
    let n = i32_at(mdl, 240).unwrap_or(0).clamp(0, 512) as usize;
    let idx = i32_at(mdl, 244).unwrap_or(0).max(0) as usize;
    let mut out = vec![];
    for i in 0..n {
        let o = idx + i * 92;
        let mut m = [0f64; 12];
        for (k, v) in m.iter_mut().enumerate() {
            let Some(f) = f32_at(mdl, o + 12 + k * 4) else { return out };
            *v = f as f64;
        }
        out.push(Attachment {
            name: rel_str(mdl, o, o),
            bone: i32_at(mdl, o + 8).unwrap_or(0).max(0) as usize,
            local: DMat4::from_cols_array(&[m[0], m[4], m[8], 0.0, m[1], m[5], m[9], 0.0, m[2], m[6], m[10], 0.0, m[3], m[7], m[11], 1.0]),
        });
    }
    out
}

fn read_sequences(src: &AnimSrc, si: usize, out: &mut Vec<Sequence>) -> Option<()> {
    let mdl = &src.mdl;
    let num_anim = i32_at(mdl, 180)?.max(0) as usize;
    let anim_idx = i32_at(mdl, 184)? as usize;
    let num_seq = i32_at(mdl, 188)?.clamp(0, 4096) as usize;
    let seq_idx = i32_at(mdl, 192)? as usize;
    for s in 0..num_seq {
        let sd = seq_idx + s * 212;
        let aii = i32_at(mdl, sd + 60)? as usize;
        let anim = i16_at(mdl, sd + aii)?.max(0) as usize;
        if anim >= num_anim {
            continue;
        }
        let ad = anim_idx + anim * 100;
        let wl = i32_at(mdl, sd + 156)?;
        let weights = (0..src.bones.len()).map(|b| if wl > 0 { f32_at(mdl, sd + wl as usize + b * 4).unwrap_or(1.0) } else { 1.0 }).collect();
        out.push(Sequence {
            name: rel_str(mdl, sd, sd + 4),
            activity: rel_str(mdl, sd, sd + 8),
            fps: f32_at(mdl, ad + 8)? as f64,
            frames: i32_at(mdl, ad + 16)?.max(1) as usize,
            looping: i32_at(mdl, sd + 12)? & 1 != 0,
            src: si,
            anim,
            weights,
        });
    }
    Some(())
}

fn skin_vert(mats: &[DMat4], v: &SkinVert) -> ModelVert {
    let n = (v.nbones as usize).clamp(1, 3);
    let (mut p, mut q) = (DVec3::ZERO, DVec3::ZERO);
    for k in 0..n {
        let w = if n == 1 { 1.0 } else { v.weights[k] as f64 };
        let m = mats.get(v.bones[k] as usize).unwrap_or(&DMat4::IDENTITY);
        p += m.transform_point3(v.pos) * w;
        q += m.transform_vector3(v.nrm) * w;
    }
    let q = q.normalize_or_zero();
    ModelVert { pos: [p.x as f32, p.y as f32, p.z as f32], nrm: [q.x as f32, q.y as f32, q.z as f32], uv: v.uv }
}

impl Model {
    pub fn find_sequence(&self, name: &str) -> Option<usize> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        self.sequences.iter().position(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Skinning matrices (bone-to-model times poseToBone) for `seq` at a fractional frame.
    pub fn skin_matrices(&self, seq: usize, frame: f64) -> Option<Vec<DMat4>> {
        let (to_model, skin) = self.pose(seq, frame)?;
        drop(to_model);
        Some(skin)
    }

    pub fn bone_count(&self) -> usize {
        self.bones.len()
    }

    pub fn bone_name(&self, i: usize) -> &str {
        self.bones.get(i).map(|b| b.name.as_str()).unwrap_or("")
    }

    pub fn bone_parent(&self, i: usize) -> Option<usize> {
        usize::try_from(self.bones.get(i)?.parent).ok()
    }

    /// Model-space position of bone `i` in a pose from `pose`.
    pub fn attachment_matrix(&self, a: &Attachment, to_model: &[DMat4]) -> DMat4 {
        to_model.get(a.bone).copied().unwrap_or(DMat4::IDENTITY) * a.local
    }

    /// (bone-to-model, skinning) matrices for `seq` at a fractional frame.
    pub fn pose(&self, seq: usize, frame: f64) -> Option<(Vec<DMat4>, Vec<DMat4>)> {
        let s = self.sequences.get(seq)?;
        let src = self.srcs.get(s.src)?;
        let mut local: Vec<(DVec3, DQuat)> = self.bones.iter().map(|b| (b.pos, b.quat)).collect();
        let f0 = frame.floor().max(0.0) as usize;
        let f1 = (f0 + 1).min(s.frames.saturating_sub(1));
        let t = (frame - f0 as f64).clamp(0.0, 1.0);
        if let Some(a) = decode_frame(src, s.anim, f0) {
            let b = if t > 1e-6 && f1 != f0 { decode_frame(src, s.anim, f1) } else { None };
            for (i, (p, q)) in a.into_iter().enumerate() {
                let Some(Some(dst)) = src.map.get(i) else { continue };
                if s.weights.get(i).copied().unwrap_or(1.0) <= 0.0 {
                    continue;
                }
                local[*dst] = match &b {
                    Some(b) => (p.lerp(b[i].0, t), q.slerp(b[i].1, t)),
                    None => (p, q),
                };
            }
        }
        let mut to_model: Vec<DMat4> = Vec::with_capacity(self.bones.len());
        let mut skin = Vec::with_capacity(self.bones.len());
        for (b, bone) in self.bones.iter().enumerate() {
            let (p, q) = local[b];
            let l = DMat4::from_rotation_translation(q.normalize(), p);
            let m = match usize::try_from(bone.parent).ok().and_then(|p| to_model.get(p)) {
                Some(pm) => *pm * l,
                None => l,
            };
            to_model.push(m);
            skin.push(m * bone.pose_to_bone);
        }
        Some((to_model, skin))
    }

    /// Triangle lists of every part posed at `seq`/`frame` (falls back to the default pose).
    pub fn posed(&self, seq: usize, frame: f64) -> Vec<Vec<ModelVert>> {
        match self.skin_matrices(seq, frame) {
            Some(m) => self.parts.iter().map(|p| p.bind.iter().map(|v| skin_vert(&m, v)).collect()).collect(),
            None => self.parts.iter().map(|p| p.verts.clone()).collect(),
        }
    }
}

/// Local (pos, quat) of every bone of `src` at an integer frame of animation `anim`.
fn decode_frame(src: &AnimSrc, anim: usize, frame: usize) -> Option<Vec<(DVec3, DQuat)>> {
    let mdl = &src.mdl;
    let ad = i32_at(mdl, 184)? as usize + anim * 100;
    let frames = i32_at(mdl, ad + 16)?.max(1) as usize;
    let frame_anim = i32_at(mdl, ad + 12)? & 0x40 != 0;
    let mut frame = frame.min(frames - 1);
    let mut block = i32_at(mdl, ad + 52)?;
    let mut index = i32_at(mdl, ad + 56)? as i64;
    let sec_frames = i32_at(mdl, ad + 84)?.max(0) as usize;
    if sec_frames > 0 {
        let s = frame / sec_frames;
        let sec = ad + i32_at(mdl, ad + 80)? as usize + s * 8;
        block = i32_at(mdl, sec)?;
        index = i32_at(mdl, sec + 4)? as i64;
        frame -= s * sec_frames;
    }
    let (data, start): (&[u8], usize) = if block == 0 {
        (mdl, (ad as i64 + index) as usize)
    } else {
        let blocks = i32_at(mdl, 356)? as usize;
        let ds = i32_at(mdl, blocks + block as usize * 8)? as i64;
        (src.ani.as_deref()?, (ds + index) as usize)
    };

    let mut out: Vec<(DVec3, DQuat)> = src.bones.iter().map(|b| (b.pos, b.quat)).collect();
    if frame_anim {
        decode_frame_anim(data, start, frame, &mut out)?;
        return Some(out);
    }
    // value of an RLE-compressed track (mstudioanim_valueptr_t) at `frame`
    let track = |vp: usize, k: usize| -> Option<f64> {
        let off = i16_at(data, vp + k * 2)?;
        if off == 0 {
            return Some(0.0);
        }
        let mut v = (vp as i64 + off as i64) as usize;
        let mut f = frame;
        loop {
            let valid = *data.get(v)? as usize;
            let total = *data.get(v + 1)? as usize;
            if total == 0 {
                return Some(0.0);
            }
            if total > f {
                let i = if valid > f { f + 1 } else { valid };
                return Some(i16_at(data, v + i * 2)? as f64);
            }
            f -= total;
            v += (valid + 1) * 2;
        }
    };
    let mut p = start;
    for _ in 0..src.bones.len() {
        let bone = *data.get(p)? as usize;
        let flags = *data.get(p + 1)?;
        let next = i16_at(data, p + 2)?;
        if let Some(b) = src.bones.get(bone) {
            let delta = flags & 0x10 != 0;
            let mut d = p + 4;
            if flags & 0x20 != 0 {
                // Quaternion64
                let raw = u64::from_le_bytes(data.get(d..d + 8)?.try_into().ok()?);
                let c = |s: u32| (((raw >> s) & 0x1f_ffff) as f64 - 1048576.0) / 1048576.5;
                let (x, y, z) = (c(0), c(21), c(42));
                let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
                out[bone].1 = DQuat::from_xyzw(x, y, z, if raw >> 63 != 0 { -w } else { w });
                d += 8;
            } else if flags & 0x02 != 0 {
                out[bone].1 = quat48(data, d)?;
                d += 6;
            } else if flags & 0x08 != 0 {
                let mut e = DVec3::new(track(d, 0)?, track(d, 1)?, track(d, 2)?) * b.rotscale;
                if !delta {
                    e += b.rot;
                }
                out[bone].1 = euler_quat(e);
                d += 6;
            }
            if flags & 0x01 != 0 {
                out[bone].0 = vec48(data, d)?;
            } else if flags & 0x04 != 0 {
                let mut v = DVec3::new(track(d, 0)?, track(d, 1)?, track(d, 2)?) * b.posscale;
                if !delta {
                    v += b.pos;
                }
                out[bone].0 = v;
            }
        }
        if next == 0 {
            break;
        }
        p = (p as i64 + next as i64) as usize;
    }
    Some(out)
}

fn quat48(b: &[u8], d: usize) -> Option<DQuat> {
    let x = (u16_at(b, d)? as f64 - 32768.0) / 32768.0;
    let y = (u16_at(b, d + 2)? as f64 - 32768.0) / 32768.0;
    let zw = u16_at(b, d + 4)?;
    let z = ((zw & 0x7fff) as f64 - 16384.0) / 16384.0;
    let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
    Some(DQuat::from_xyzw(x, y, z, if zw & 0x8000 != 0 { -w } else { w }))
}

fn vec48(b: &[u8], d: usize) -> Option<DVec3> {
    let h = |k: usize| u16_at(b, d + k * 2).map(f16_to_f64);
    Some(DVec3::new(h(0)?, h(1)?, h(2)?))
}

/// mstudio_frame_anim_t: per-bone flags, a constants stream and fixed-size per-frame records.
fn decode_frame_anim(data: &[u8], start: usize, frame: usize, out: &mut [(DVec3, DQuat)]) -> Option<()> {
    let consts = start + i32_at(data, start)? as usize;
    let frame_off = i32_at(data, start + 4)? as usize;
    let frame_len = i32_at(data, start + 8)? as usize;
    let flags_at = start + 24;
    let mut c = consts;
    let mut f = start + frame_off + frame * frame_len;
    for (i, slot) in out.iter_mut().enumerate() {
        let flags = *data.get(flags_at + i)?;
        if flags & 0x02 != 0 {
            slot.1 = quat48(data, c)?;
            c += 6;
        } else if flags & 0x08 != 0 {
            slot.1 = quat48(data, f)?;
            f += 6;
        }
        if flags & 0x01 != 0 {
            slot.0 = vec48(data, c)?;
            c += 6;
        } else if flags & 0x04 != 0 {
            slot.0 = vec48(data, f)?;
            f += 6;
        } else if flags & 0x10 != 0 {
            slot.0 = vec3_at(data, f)?;
            f += 12;
        }
    }
    Some(())
}

fn build_meshes(mdl: &[u8], vvd: &[u8], vtx: &[u8], fs: &GameFs, model: &mut Model) -> Option<()> {
    // ---- material names ----
    let num_tex = i32_at(mdl, 204)? as usize;
    let tex_idx = i32_at(mdl, 208)? as usize;
    let num_cd = i32_at(mdl, 212)? as usize;
    let cd_idx = i32_at(mdl, 216)? as usize;
    let num_skinref = i32_at(mdl, 220)? as usize;
    let num_families = i32_at(mdl, 224)? as usize;
    let skin_idx = i32_at(mdl, 228)? as usize;
    let mut cds: Vec<String> = vec![];
    for i in 0..num_cd {
        let off = i32_at(mdl, cd_idx + i * 4)? as usize;
        cds.push(cstr_at(mdl, off).replace('\\', "/").to_ascii_lowercase());
    }
    let mut tex_names: Vec<String> = vec![];
    for i in 0..num_tex {
        let base = tex_idx + i * 64;
        let off = i32_at(mdl, base)?;
        tex_names.push(cstr_at(mdl, (base as i64 + off as i64) as usize).replace('\\', "/").to_ascii_lowercase());
    }
    let resolve = |name: &str| -> String {
        for cd in &cds {
            let full = format!("{}{}", cd.trim_start_matches('/'), name);
            if fs.exists(&format!("materials/{full}.vmt")) {
                return full;
            }
        }
        format!("{}{}", cds.first().cloned().unwrap_or_default(), name)
    };
    let tex_resolved: Vec<String> = tex_names.iter().map(|n| resolve(n)).collect();
    let skin_ref = |family: usize, r: usize| -> Option<usize> {
        let o = skin_idx + (family * num_skinref + r) * 2;
        Some(u16_at(mdl, o)? as usize)
    };

    // ---- VVD vertices (LOD 0, fixups applied), in the bind pose ----
    if vvd.len() < 64 || &vvd[0..4] != b"IDSV" {
        return None;
    }
    let num_lod0 = i32_at(vvd, 16)? as usize;
    let num_fix = i32_at(vvd, 48)? as usize;
    let fix_start = i32_at(vvd, 52)? as usize;
    let vert_start = i32_at(vvd, 56)? as usize;
    let read_vert = |i: usize| -> Option<SkinVert> {
        let o = vert_start + i * 48;
        Some(SkinVert {
            pos: vec3_at(vvd, o + 16)?,
            nrm: vec3_at(vvd, o + 28)?,
            uv: [f32_at(vvd, o + 40)?, f32_at(vvd, o + 44)?],
            weights: [f32_at(vvd, o)?, f32_at(vvd, o + 4)?, f32_at(vvd, o + 8)?],
            bones: [*vvd.get(o + 12)?, *vvd.get(o + 13)?, *vvd.get(o + 14)?],
            nbones: *vvd.get(o + 15)?,
        })
    };
    let mut verts: Vec<SkinVert> = Vec::new();
    if num_fix == 0 {
        for i in 0..num_lod0 {
            verts.push(read_vert(i)?);
        }
    } else {
        for f in 0..num_fix {
            let o = fix_start + f * 12;
            let lod = i32_at(vvd, o)?;
            let src = i32_at(vvd, o + 4)? as usize;
            let cnt = i32_at(vvd, o + 8)? as usize;
            if lod >= 0 {
                for i in 0..cnt {
                    verts.push(read_vert(src + i)?);
                }
            }
        }
    }

    // ---- walk MDL body parts and the matching VTX hierarchy ----
    let num_bp = i32_at(mdl, 232)? as usize;
    let bp_idx = i32_at(mdl, 236)? as usize;
    let vtx_bp_count = i32_at(vtx, 28)? as usize;
    let vtx_bp_off = i32_at(vtx, 32)? as usize;
    for b in 0..num_bp.min(vtx_bp_count) {
        let bp = bp_idx + b * 16;
        let num_models = i32_at(mdl, bp + 4)? as usize;
        let model_index = i32_at(mdl, bp + 12)? as usize;
        if num_models == 0 {
            continue;
        }
        // first model of each body part
        let mbase = bp + model_index;
        let num_meshes = i32_at(mdl, mbase + 72)? as usize;
        let mesh_index = i32_at(mdl, mbase + 76)? as usize;
        let vertex_index = i32_at(mdl, mbase + 84)? as usize / 48;

        let vbp = vtx_bp_off + b * 8;
        let v_models_off = i32_at(vtx, vbp + 4)? as usize;
        let vmodel = vbp + v_models_off;
        let v_lod_off = i32_at(vtx, vmodel + 4)? as usize;
        let vlod = vmodel + v_lod_off; // LOD 0
        let v_mesh_count = i32_at(vtx, vlod)? as usize;
        let v_mesh_off = i32_at(vtx, vlod + 4)? as usize;

        for k in 0..num_meshes.min(v_mesh_count) {
            let mesh = mbase + mesh_index + k * 116;
            let mat_ref = i32_at(mdl, mesh)? as usize;
            let vert_offset = i32_at(mdl, mesh + 12)? as usize;
            let vmesh = vlod + v_mesh_off + k * 9;
            let n_groups = i32_at(vtx, vmesh)? as usize;
            let g_off = i32_at(vtx, vmesh + 4)? as usize;

            let mut part = ModelPart::default();
            for fam in 0..num_families.max(1) {
                let tex = skin_ref(fam, mat_ref).unwrap_or(mat_ref);
                part.materials.push(tex_resolved.get(tex).cloned().unwrap_or_default());
            }
            for g in 0..n_groups {
                let sg = vmesh + g_off + g * 25;
                let n_verts = i32_at(vtx, sg)? as usize;
                let vert_off = i32_at(vtx, sg + 4)? as usize;
                let n_idx = i32_at(vtx, sg + 8)? as usize;
                let idx_off = i32_at(vtx, sg + 12)? as usize;
                let mut group_verts: Vec<usize> = Vec::with_capacity(n_verts);
                for i in 0..n_verts {
                    let o = sg + vert_off + i * 9;
                    let orig = u16_at(vtx, o + 4)? as usize;
                    group_verts.push(vertex_index + vert_offset + orig);
                }
                let mut tri = [0usize; 3];
                for i in 0..n_idx {
                    let gi = u16_at(vtx, sg + idx_off + i * 2)? as usize;
                    tri[i % 3] = *group_verts.get(gi)?;
                    if i % 3 == 2 {
                        for t in tri {
                            let v = *verts.get(t)?;
                            part.verts.push(ModelVert {
                                pos: [v.pos.x as f32, v.pos.y as f32, v.pos.z as f32],
                                nrm: [v.nrm.x as f32, v.nrm.y as f32, v.nrm.z as f32],
                                uv: v.uv,
                            });
                            part.bind.push(v);
                        }
                    }
                }
            }
            if !part.verts.is_empty() {
                model.parts.push(part);
            }
        }
    }
    Some(())
}
