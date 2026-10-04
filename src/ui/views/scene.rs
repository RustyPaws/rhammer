//! Scene preparation: world / model / overlay meshes for the 3D view.

use crate::app::*;
use crate::editor::doc::Obj;
use crate::editor::geom;
use crate::render3d::{Batch, Vertex};
use glam::DVec3;
use std::collections::HashMap;
use super::*;

impl App {
    pub(crate) fn material_color(&mut self, mat: &str) -> ([f32; 4], Option<(u32, u32)>) {
        let lower = mat.to_ascii_lowercase();
        if let Some(info) = self.mats.get(&lower) {
            return ([1.0, 1.0, 1.0, 1.0], Some((info.w, info.h)));
        }
        let mut h: u32 = 2166136261;
        for b in lower.bytes() {
            h = (h ^ b as u32).wrapping_mul(16777619);
        }
        let g = 0.45 + (h & 0xff) as f32 / 255.0 * 0.4;
        let tint = ((h >> 8) & 0xff) as f32 / 255.0 * 0.15;
        ([g + tint, g, g - tint * 0.5, 1.0], None)
    }

    /// Rebuild world mesh / overlay when the document or selection changed.
    pub(crate) fn prepare_scene(&mut self) {
        self.mats.budget = 12;
        self.mats.starved = false;
        if self.inst_key != self.doc.version {
            self.rebuild_instances();
            self.inst_key = if self.inst_cache.starved { u64::MAX } else { self.doc.version };
            self.world_key = (u64::MAX, 0);
            self.models_key = u64::MAX; // instance props may need loading
            if self.inst_cache.starved {
                self.ctx.request_repaint();
            }
        }
        if self.model_starved || self.models_key != self.doc.version {
            self.ensure_models();
            self.models_key = self.doc.version;
            self.world_key = (u64::MAX, 0);
        }
        let key = (self.doc.version, self.mats.info.len() + self.models.len());
        if self.world_key.0 != self.doc.version || self.world_key.1 != self.mats.info.len() + self.models.len() {
            self.rebuild_world();
            if self.mats.starved || self.model_starved {
                self.world_key = (self.doc.version, usize::MAX);
                self.ctx.request_repaint();
            } else {
                self.world_key = key;
            }
        }
        if self.anim_play && self.anim_active {
            self.anim_time += self.ctx.input(|i| i.stable_dt).min(0.1) as f64;
            self.ctx.request_repaint();
        }
        let mkey = (self.doc.version, self.mats.info.len() + self.models.len() + self.inst.len(), self.anim_stamp, if self.anim_active { self.anim_time.to_bits() } else { 0 });
        if self.anim_key != mkey {
            self.rebuild_models();
            self.anim_key = mkey;
        }
        let okey = (self.doc.version, self.sel_stamp, self.faces.len() as u64 + (self.faces.iter().next().copied().unwrap_or(0) as u64) * 1000);
        if self.overlay_key != okey {
            self.rebuild_overlay();
            self.overlay_key = okey;
        }
        self.ensure_sprites();
        if let Ok(mut sh) = self.shared.lock() {
            sh.scene.uploads.append(&mut self.mats.ready);
        }
    }

    /// Load (a few per frame) the icon sprites of point entities as egui textures for the 2D views.
    pub(crate) fn ensure_sprites(&mut self) {
        if self.sprites_key == self.doc.version && !self.sprites_pending {
            return;
        }
        let mut names: Vec<String> =
            self.doc.map.entities.iter().filter(|e| e.solids.is_empty()).filter_map(|e| crate::editor::doc::entity_sprite(e, &self.fgd)).collect();
        names.sort_unstable();
        names.dedup();
        self.sprites_pending = false;
        let mut loads = 4;
        for n in names {
            if self.sprite_tex.contains_key(&n) {
                continue;
            }
            if loads == 0 {
                self.sprites_pending = true;
                break;
            }
            loads -= 1;
            match self.mats.thumb(&n) {
                Some((w, h, rgba)) => {
                    let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                    let t = self.ctx.load_texture(format!("sprite:{n}"), img, egui::TextureOptions::LINEAR);
                    self.sprite_tex.insert(n, Some(t));
                }
                None if self.mats.loading() => self.sprites_pending = true,
                None => {
                    self.sprite_tex.insert(n, None);
                }
            }
        }
        self.sprites_key = self.doc.version;
        if self.sprites_pending {
            self.ctx.request_repaint();
        }
    }

    /// Load (a few per frame) the models referenced by entities and instances, and publish their hulls.
    pub(crate) fn ensure_models(&mut self) {
        self.model_budget = 6;
        self.model_starved = false;
        let mut paths: Vec<String> = self
            .doc
            .map
            .entities
            .iter()
            .filter(|e| e.solids.is_empty())
            .filter_map(|e| crate::editor::doc::entity_model(e, &self.fgd))
            .collect();
        paths.extend(self.inst.values().flat_map(|ig| ig.props.iter().map(|p| p.model.clone())));
        paths.sort_unstable();
        paths.dedup();
        for p in paths {
            if !self.models.contains_key(&p) {
                if self.model_budget <= 0 {
                    self.model_starved = true;
                    continue;
                }
                self.model_budget -= 1;
                let mark = self.mats.fs.mark();
                let m = crate::assets::mdl::load(&self.mats.fs, &p).map(std::rc::Rc::new);
                if m.is_none() && self.mats.fs.stalled_since(mark) {
                    self.model_starved = true;
                    continue;
                }
                self.models.insert(p.clone(), m);
            }
            if let Some(Some(m)) = self.models.get(&p) {
                self.doc.model_bounds.insert(p, (m.hull_min, m.hull_max));
            }
        }
        // grow instance bounds by the hulls of the props they contain
        for (id, ig) in &self.inst {
            let Some(b) = self.doc.inst_bounds.get_mut(id) else { continue };
            for p in &ig.props {
                let Some(Some(m)) = self.models.get(&p.model) else { continue };
                for i in 0..8 {
                    let c = DVec3::new(
                        if i & 1 != 0 { m.hull_max.x } else { m.hull_min.x },
                        if i & 2 != 0 { m.hull_max.y } else { m.hull_min.y },
                        if i & 4 != 0 { m.hull_max.z } else { m.hull_min.z },
                    );
                    let w = p.origin + p.rot * (c * p.scale);
                    *b = (b.0.min(w), b.1.max(w));
                }
            }
        }
        if self.model_starved {
            self.ctx.request_repaint();
        }
    }

    pub(crate) fn rebuild_instances(&mut self) {
        self.inst_cache.budget = 4;
        self.inst_cache.starved = false;
        let map_dir = self.doc.path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf());
        let fallback: Vec<std::path::PathBuf> =
            self.game().filter(|g| !g.map_dir.is_empty()).map(|g| std::path::PathBuf::from(&g.map_dir)).into_iter().collect();
        let jobs: Vec<(u32, String, DVec3, DVec3)> = self
            .doc
            .map
            .entities
            .iter()
            .filter(|e| e.classname() == "func_instance" && !self.doc.is_hidden(e.id))
            .filter_map(|e| e.get("file").map(|f| (e.id, f.to_string(), e.origin(), e.angles())))
            .collect();
        let mut out = HashMap::new();
        let mut bounds = HashMap::new();
        for (id, file, o, a) in jobs {
            if let Some(g) = crate::editor::instances::instance_geo(&mut self.inst_cache, &self.fgd, &file, o, a, map_dir.as_deref(), &fallback) {
                bounds.insert(id, (g.min, g.max));
                out.insert(id, g);
            }
        }
        self.inst = out;
        self.doc.inst_bounds = bounds;
    }

    pub(crate) fn rebuild_world(&mut self) {
        let mut batches: HashMap<String, Vec<Vertex>> = HashMap::new();
        let mut lines: Vec<Vertex> = Vec::new();
        let mut colored: Vec<Vertex> = Vec::new();
        let solids: Vec<(u32, bool)> = {
            let mut v: Vec<(u32, bool)> = vec![];
            for s in &self.doc.map.world.solids {
                if !self.doc.is_hidden(s.id) {
                    v.push((s.id, false));
                }
            }
            v
        };
        let mut jobs: Vec<(Vec<crate::formats::vmf::Side>, Vec<Vec<DVec3>>, bool)> = Vec::new();
        for (id, _) in &solids {
            if let (Some(Obj::WorldSolid(i)), Some(g)) = (self.doc.index.get(id), self.doc.geo.get(id)) {
                jobs.push((self.doc.map.world.solids[*i].sides.clone(), g.polys.clone(), false));
            }
        }
        let mut point_boxes: Vec<(DVec3, DVec3, [f32; 4])> = vec![];
        let mut sprite_cands: Vec<(String, DVec3, DVec3, [f32; 4])> = vec![];
        for e in &self.doc.map.entities {
            if self.doc.is_hidden(e.id) {
                continue;
            }
            if e.solids.is_empty() {
                if self.inst.contains_key(&e.id) {
                    continue; // drawn through its instance geometry
                }
                if let Some(m) = crate::editor::doc::entity_model(e, &self.fgd) {
                    if let Some(Some(model)) = self.models.get(&m) {
                        if !model.parts.is_empty() {
                            continue; // drawn by rebuild_models
                        }
                    }
                }
                let (a, b) = self.doc.ent_bounds(e, &self.fgd);
                let c = ent_color(e, &self.fgd);
                let col = [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0];
                match crate::editor::doc::entity_sprite(e, &self.fgd) {
                    // decided below: needs the material loaded (mutable access)
                    Some(sp) => sprite_cands.push((sp, a, b, col)),
                    None => point_boxes.push((a, b, col)),
                }
            } else {
                for s in &e.solids {
                    if let Some(g) = self.doc.geo.get(&s.id) {
                        jobs.push((s.sides.clone(), g.polys.clone(), true));
                    }
                }
            }
        }
        for (sides, polys, _is_ent) in jobs {
            for (sd, poly) in sides.iter().zip(&polys) {
                if poly.len() < 3 {
                    continue;
                }
                let (col, size) = self.material_color(&sd.material);
                let (tw, th) = size.map(|(w, h)| (w as f64, h as f64)).unwrap_or((64.0, 64.0));
                let n = geom::Plane::from_points(&sd.plane).map(|p| p.n).unwrap_or(DVec3::Z);
                let verts: Vec<Vertex> = poly
                    .iter()
                    .map(|p| {
                        let u = (p.dot(sd.uaxis.vec) / sd.uaxis.scale + sd.uaxis.shift) / tw;
                        let v = (p.dot(sd.vaxis.vec) / sd.vaxis.scale + sd.vaxis.shift) / th;
                        Vertex { pos: [p.x as f32, p.y as f32, p.z as f32], nrm: [n.x as f32, n.y as f32, n.z as f32], uv: [u as f32, v as f32], col }
                    })
                    .collect();
                let out = batches.entry(sd.material.to_ascii_lowercase()).or_default();
                for i in 1..verts.len() - 1 {
                    out.push(verts[0]);
                    out.push(verts[i]);
                    out.push(verts[i + 1]);
                }
            }
        }
        // instance geometry
        let inst = std::mem::take(&mut self.inst);
        for ig in inst.values() {
            for f in &ig.faces {
                let (col, size) = self.material_color(&f.material);
                let (tw, th) = size.map(|(w, h)| (w as f64, h as f64)).unwrap_or((64.0, 64.0));
                let n = f.rot * f.normal;
                let verts: Vec<Vertex> = f
                    .local
                    .iter()
                    .map(|p| {
                        let u = (p.dot(f.uaxis.vec) / f.uaxis.scale + f.uaxis.shift) / tw;
                        let v = (p.dot(f.vaxis.vec) / f.vaxis.scale + f.vaxis.shift) / th;
                        let w = f.rot * *p + f.trans;
                        Vertex { pos: [w.x as f32, w.y as f32, w.z as f32], nrm: [n.x as f32, n.y as f32, n.z as f32], uv: [u as f32, v as f32], col }
                    })
                    .collect();
                let out = batches.entry(f.material.to_ascii_lowercase()).or_default();
                for i in 1..verts.len() - 1 {
                    out.push(verts[0]);
                    out.push(verts[i]);
                    out.push(verts[i + 1]);
                }
            }
        }
        self.inst = inst;
        // entity icons: two crossed upright quads, textured with the sprite material
        let mut sprites: Vec<(String, DVec3, f64)> = vec![];
        for (sp, a, b, col) in sprite_cands {
            if self.material_color(&sp).1.is_some() {
                sprites.push((sp, (a + b) * 0.5, (b - a).max_element().max(16.0) * 0.5));
            } else {
                point_boxes.push((a, b, col)); // no texture: keep the plain cube
            }
        }
        for (mat, c, r) in sprites {
            let out = batches.entry(mat).or_default();
            let pos = [c.x as f32, c.y as f32, c.z as f32];
            // a negative alpha makes the vertex shader expand the corner (nrm.xy * nrm.z) along the camera axes
            let v = |cx: f32, cy: f32| Vertex { pos, nrm: [cx, cy, r as f32], uv: [(cx + 1.0) * 0.5, (1.0 - cy) * 0.5], col: [1.0, 1.0, 1.0, -1.0] };
            let q = [v(-1.0, -1.0), v(1.0, -1.0), v(1.0, 1.0), v(-1.0, 1.0)];
            out.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
        }
        for (a, b, col) in point_boxes {
            let faces: [([usize; 4], [f32; 3]); 6] = [
                ([0, 1, 3, 2], [-1.0, 0.0, 0.0]),
                ([4, 6, 7, 5], [1.0, 0.0, 0.0]),
                ([0, 4, 5, 1], [0.0, -1.0, 0.0]),
                ([2, 3, 7, 6], [0.0, 1.0, 0.0]),
                ([0, 2, 6, 4], [0.0, 0.0, -1.0]),
                ([1, 5, 7, 3], [0.0, 0.0, 1.0]),
            ];
            let corner = |i: usize| -> [f32; 3] {
                [
                    if i & 4 != 0 { b.x } else { a.x } as f32,
                    if i & 2 != 0 { b.y } else { a.y } as f32,
                    if i & 1 != 0 { b.z } else { a.z } as f32,
                ]
            };
            for (idx, n) in faces {
                let vs: Vec<Vertex> = idx.iter().map(|&i| Vertex { pos: corner(i), nrm: n, uv: [0.0, 0.0], col }).collect();
                colored.extend([vs[0], vs[1], vs[2], vs[0], vs[2], vs[3]]);
            }
            let edges = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
            for (i, j) in edges {
                for k in [i, j] {
                    lines.push(Vertex { pos: corner(k), nrm: [0.0, 0.0, 1.0], uv: [0.0; 2], col: [0.0, 0.0, 0.0, 1.0] });
                }
            }
        }
        let mut list: Vec<Batch> = batches.into_iter().map(|(material, verts)| Batch { material, verts }).collect();
        list.push(Batch { material: String::new(), verts: colored });
        if let Ok(mut sh) = self.shared.lock() {
            sh.scene.batches = list;
            sh.scene.world_version += 1;
            // point entity outlines live in `lines`, merged with overlay lines on rebuild_overlay
            sh.scene.world_lines = lines;
        }
        self.overlay_key = (u64::MAX, 0, 0);
    }

    /// Sequence shown for an entity: editor preview, then DefaultAnim, then the `sequence` key.
    pub fn entity_sequence(&self, e: &crate::formats::vmf::Entity, m: &crate::assets::mdl::Model) -> usize {
        if let Some(s) = self.anim_preview.get(&e.id).filter(|s| **s < m.sequences.len()) {
            return *s;
        }
        e.get("DefaultAnim")
            .and_then(|n| m.find_sequence(n))
            .or_else(|| e.get("sequence").and_then(|s| s.trim().parse().ok()).filter(|i| *i < m.sequences.len()))
            .unwrap_or(0)
    }

    /// Pose and batch the studio models of point entities.
    pub(crate) fn rebuild_models(&mut self) {
        let mut jobs: Vec<(std::rc::Rc<crate::assets::mdl::Model>, DVec3, glam::DMat3, usize, f64, usize, f64)> = vec![];
        let mut active = false;
        for e in &self.doc.map.entities {
            if !e.solids.is_empty() || self.doc.is_hidden(e.id) || self.inst.contains_key(&e.id) {
                continue;
            }
            let Some(path) = crate::editor::doc::entity_model(e, &self.fgd) else { continue };
            let Some(Some(model)) = self.models.get(&path) else { continue };
            if model.parts.is_empty() {
                continue;
            }
            let seq = self.entity_sequence(e, model);
            let frame = match model.sequences.get(seq) {
                Some(s) if s.frames > 1 => {
                    active = true;
                    s.frame_at(self.anim_time)
                }
                _ => 0.0,
            };
            let skin: usize = e.get("skin").and_then(|s| s.parse().ok()).unwrap_or(0);
            let scale: f64 = e.get("modelscale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
            jobs.push((model.clone(), e.origin(), crate::editor::doc::angles_matrix(e.angles()), skin, scale, seq, frame));
        }
        // props inside instances, shown in their bind pose
        for (id, ig) in &self.inst {
            if self.doc.is_hidden(*id) {
                continue;
            }
            for p in &ig.props {
                if let Some(Some(model)) = self.models.get(&p.model) {
                    if !model.parts.is_empty() {
                        jobs.push((model.clone(), p.origin, p.rot, p.skin, p.scale, 0, 0.0));
                    }
                }
            }
        }
        self.anim_active = active;
        let mut batches: HashMap<String, Vec<Vertex>> = HashMap::new();
        for (model, origin, rot, skin, scale, seq, frame) in jobs {
            let posed;
            let lists: Vec<&[crate::assets::mdl::ModelVert]> = if seq == 0 && frame == 0.0 {
                model.parts.iter().map(|p| &p.verts[..]).collect()
            } else {
                posed = model.posed(seq, frame);
                posed.iter().map(|v| &v[..]).collect()
            };
            for (part, verts) in model.parts.iter().zip(lists) {
                let mat = part.materials.get(skin).or(part.materials.first()).cloned().unwrap_or_default();
                let (col, _) = self.material_color(&mat);
                let out = batches.entry(mat.to_ascii_lowercase()).or_default();
                let conv = |v: &crate::assets::mdl::ModelVert| {
                    let p = origin + rot * (DVec3::new(v.pos[0] as f64, v.pos[1] as f64, v.pos[2] as f64) * scale);
                    let n = rot * DVec3::new(v.nrm[0] as f64, v.nrm[1] as f64, v.nrm[2] as f64);
                    Vertex { pos: [p.x as f32, p.y as f32, p.z as f32], nrm: [n.x as f32, n.y as f32, n.z as f32], uv: v.uv, col }
                };
                // Source triangles are clockwise; flip to CCW for culling
                for tri in verts.chunks_exact(3) {
                    out.push(conv(&tri[0]));
                    out.push(conv(&tri[2]));
                    out.push(conv(&tri[1]));
                }
            }
        }
        if let Ok(mut sh) = self.shared.lock() {
            sh.scene.model_batches = batches.into_iter().map(|(material, verts)| Batch { material, verts }).collect();
            sh.scene.models_version += 1;
        }
    }

    pub(crate) fn rebuild_overlay(&mut self) {
        let mut tris: Vec<Vertex> = vec![];
        let mut lines: Vec<Vertex> = vec![];
        let sel_col = [1.0, 0.2, 0.2, 0.28];
        let line_col = [1.0, 0.25, 0.25, 1.0];
        let face_col = [1.0, 0.9, 0.2, 0.45];
        let mk = |p: DVec3, c: [f32; 4]| Vertex { pos: [p.x as f32, p.y as f32, p.z as f32], nrm: [0.0, 0.0, 1.0], uv: [0.0; 2], col: c };
        for id in &self.sel {
            for s in self.doc.solids_of(*id) {
                if let Some(g) = self.doc.geo.get(&s.id) {
                    for poly in &g.polys {
                        if poly.len() < 3 {
                            continue;
                        }
                        for i in 1..poly.len() - 1 {
                            tris.push(mk(poly[0], sel_col));
                            tris.push(mk(poly[i], sel_col));
                            tris.push(mk(poly[i + 1], sel_col));
                        }
                        for i in 0..poly.len() {
                            lines.push(mk(poly[i], line_col));
                            lines.push(mk(poly[(i + 1) % poly.len()], line_col));
                        }
                    }
                }
            }
            if let Some(ig) = self.inst.get(id) {
                for poly in &ig.polys {
                    if poly.len() < 3 {
                        continue;
                    }
                    for i in 1..poly.len() - 1 {
                        tris.push(mk(poly[0], sel_col));
                        tris.push(mk(poly[i], sel_col));
                        tris.push(mk(poly[i + 1], sel_col));
                    }
                    for i in 0..poly.len() {
                        lines.push(mk(poly[i], line_col));
                        lines.push(mk(poly[(i + 1) % poly.len()], line_col));
                    }
                }
            }
            if let Some(e) = self.doc.entity(*id) {
                if e.solids.is_empty() {
                    let (a, b) = self.doc.ent_bounds(e, &self.fgd);
                    let c = |i: usize| DVec3::new(if i & 4 != 0 { b.x } else { a.x }, if i & 2 != 0 { b.y } else { a.y }, if i & 1 != 0 { b.z } else { a.z });
                    for (i, j) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                        lines.push(mk(c(i), line_col));
                        lines.push(mk(c(j), line_col));
                    }
                }
            }
        }
        if !self.faces.is_empty() {
            let all: Vec<&crate::formats::vmf::Solid> = self.doc.map.world.solids.iter().chain(self.doc.map.entities.iter().flat_map(|e| e.solids.iter())).collect();
            for s in all {
                if let Some(g) = self.doc.geo.get(&s.id) {
                    for (sd, poly) in s.sides.iter().zip(&g.polys) {
                        if self.faces.contains(&sd.id) && poly.len() >= 3 {
                            for i in 1..poly.len() - 1 {
                                tris.push(mk(poly[0], face_col));
                                tris.push(mk(poly[i], face_col));
                                tris.push(mk(poly[i + 1], face_col));
                            }
                            for i in 0..poly.len() {
                                lines.push(mk(poly[i], [1.0, 1.0, 0.0, 1.0]));
                                lines.push(mk(poly[(i + 1) % poly.len()], [1.0, 1.0, 0.0, 1.0]));
                            }
                        }
                    }
                }
            }
        }
        if let Ok(mut sh) = self.shared.lock() {
            sh.scene.overlay_tris = tris;
            let mut l = sh.scene.world_lines.clone();
            l.extend(lines);
            sh.scene.lines = l;
            sh.scene.overlay_version += 1;
        }
    }
}
