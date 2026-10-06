//! Editing commands on the selection, undo / redo and view framing.

use super::{axes, App};
use crate::editor::doc::{Sel, Xform};
use crate::editor::geom::{self, Plane};
use glam::{DQuat, DVec3};

impl App {
    // ---- camera / view helpers ---------------------------------------------------------------

    pub fn frame_all(&mut self) {
        let ids = self.doc.all_ids();
        let all: Sel = ids.into_iter().collect();
        self.frame_sel_set(&all);
    }

    pub fn frame_selection(&mut self) {
        let s = self.sel.clone();
        self.frame_sel_set(&s);
    }

    fn frame_sel_set(&mut self, s: &Sel) {
        let Some((min, max)) = self.doc.sel_bounds(s, &self.fgd) else { return };
        let c = (min + max) * 0.5;
        let size = (max - min).max(DVec3::splat(64.0));
        for (vi, v) in self.views.iter_mut().enumerate() {
            let (u, w, _) = axes(vi);
            v.center = (c[u], c[w]);
            v.zoom = (600.0 / size[u].max(size[w])).clamp(0.02, 8.0);
        }
        let r = (max - min).length().max(128.0);
        self.cam.pos = c + DVec3::new(-r * 0.8, -r * 0.8, r * 0.6);
        let d = (c - self.cam.pos).normalize();
        self.cam.yaw = d.y.atan2(d.x).to_degrees();
        self.cam.pitch = d.z.asin().to_degrees();
    }

    // ---- editing commands ------------------------------------------------------------------

    pub fn delete_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let s = self.sel.clone();
        self.doc.delete(&s);
        self.set_sel(Sel::new());
        self.status = format!("Deleted {} objects", s.len());
    }

    pub fn copy_selection(&mut self) {
        self.clipboard = self.doc.copy_objects(&self.sel);
        self.paste_count = 0;
        self.status = format!("Copied {} objects", self.sel.len());
    }

    pub fn cut_selection(&mut self) {
        self.copy_selection();
        self.delete_selection();
    }

    pub fn paste_clipboard(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.paste_count += 1;
        let off = DVec3::splat(self.grid * self.paste_count as f64);
        let off = DVec3::new(off.x, -off.y, 0.0);
        self.doc.checkpoint();
        let cb = self.clipboard.clone();
        let n = self.doc.paste(&cb, off);
        self.set_sel(n);
        self.status = "Pasted".into();
    }

    pub fn duplicate_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let s = self.sel.clone();
        let n = self.doc.clone_objects(&s, DVec3::new(self.grid, -self.grid, 0.0));
        self.set_sel(n);
    }

    pub fn apply_xform(&mut self, xf: Xform) {
        if self.sel.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let s = self.sel.clone();
        self.doc.transform(&s, &xf, self.tex_lock);
    }

    pub fn rotate_selection(&mut self, axis: DVec3, deg: f64) {
        let Some((a, b)) = self.doc.sel_bounds(&self.sel, &self.fgd) else { return };
        let c = (a + b) * 0.5;
        let c = DVec3::new(self.snapv(c.x), self.snapv(c.y), self.snapv(c.z));
        self.apply_xform(Xform::Rotate { center: c, q: DQuat::from_axis_angle(axis, deg.to_radians()) });
    }

    pub fn mirror_selection(&mut self, axis: usize) {
        let Some((a, b)) = self.doc.sel_bounds(&self.sel, &self.fgd) else { return };
        let c = (a + b) * 0.5;
        self.apply_xform(Xform::Mirror { center: c, axis });
    }

    pub fn hollow_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let s = self.sel.clone();
        let n = self.doc.hollow(&s, self.hollow_thickness, self.tex_scale, 16);
        if !n.is_empty() {
            self.set_sel(n);
        }
    }

    pub fn tie_selection(&mut self, class: &str) {
        let s = self.sel.clone();
        self.doc.checkpoint();
        if let Some(id) = self.doc.tie_to_entity(&s, class, &self.fgd) {
            self.set_sel([id].into_iter().collect());
            self.open_properties();
        }
    }

    /// Shows the Object Properties window for the current selection.
    pub fn open_properties(&mut self) {
        self.win.object_props = true;
    }

    pub fn move_to_world(&mut self) {
        let s = self.sel.clone();
        self.doc.checkpoint();
        let n = self.doc.move_to_world(&s);
        self.set_sel(n);
    }

    pub fn commit_block(&mut self) {
        let Some((mut a, mut b)) = self.block else { return };
        for i in 0..3 {
            if a[i] > b[i] {
                std::mem::swap(&mut a[i], &mut b[i]);
            }
        }
        let planes = geom::primitive_planes(self.primitive, a, b, self.prim_sides);
        self.doc.checkpoint();
        let lm = self.game().map(|g| g.default_lightmap_scale).unwrap_or(16);
        if let Some(id) = self.doc.create_solid(&planes, &self.cur_mat.clone(), self.tex_scale, lm) {
            self.set_sel([id].into_iter().collect());
            self.block = None;
            self.status = format!("Created {}", self.primitive.name());
        } else {
            self.status = "Could not create brush (zero size?)".into();
        }
    }

    /// The clip plane of the line drawn in a view; its normal is the "front" side.
    pub fn clip_plane(&self) -> Option<Plane> {
        let (p0, p1) = (self.clip.p0?, self.clip.p1?);
        if (p0.0 - p1.0).abs() < 1e-6 && (p0.1 - p1.1).abs() < 1e-6 {
            return None;
        }
        let (ua, va, wa) = axes(self.clip.view);
        let mut a = DVec3::ZERO;
        let mut b = DVec3::ZERO;
        a[ua] = p0.0;
        a[va] = p0.1;
        b[ua] = p1.0;
        b[va] = p1.1;
        let mut wdir = DVec3::ZERO;
        wdir[wa] = 1.0;
        // normal perpendicular to the line within the view plane
        let n = (b - a).cross(wdir);
        if n.length() < 1e-9 {
            return None;
        }
        let n = n.normalize();
        Some(Plane { n, d: n.dot(a) })
    }

    /// What the clip would produce for the selected brushes: the pieces' faces and whether
    /// each piece stays (white) or is removed (red) in the current mode.
    pub fn clip_preview(&self) -> Vec<(Vec<Vec<DVec3>>, bool)> {
        let Some(pl) = self.clip_plane() else { return vec![] };
        let (kf, kb) = self.clip.mode.keeps();
        let mut next = self.doc.next_id;
        let mut out = vec![];
        for id in &self.sel {
            for s in self.doc.solids_of(*id) {
                let (front, back) = crate::editor::geom::clip_solid(s, &pl, "", &mut next);
                for (piece, keep) in [(front, kf), (back, kb)] {
                    if let Some(p) = piece {
                        out.push((crate::editor::geom::SolidGeo::build(&p).polys, keep));
                    }
                }
            }
        }
        out
    }

    pub fn commit_clip(&mut self) {
        let Some(pl) = self.clip_plane() else { return };
        let (kf, kb) = self.clip.mode.keeps();
        self.doc.checkpoint();
        let s = self.sel.clone();
        let mat = self.cur_mat.clone();
        let n = self.doc.clip(&s, &pl, kf, kb, &mat);
        self.set_sel(n);
        self.clip.p0 = None;
        self.clip.p1 = None;
    }

    pub fn undo(&mut self) {
        if self.doc.undo() {
            self.sel.retain(|id| self.doc.index.contains_key(id));
            self.vtx.sel.clear();
            self.bump_sel();
            self.status = "Undo".into();
        }
    }

    pub fn redo(&mut self) {
        if self.doc.redo() {
            self.sel.retain(|id| self.doc.index.contains_key(id));
            self.vtx.sel.clear();
            self.bump_sel();
            self.status = "Redo".into();
        }
    }

    pub fn default_solid_class(&self) -> String {
        self.game().map(|g| g.default_solid_entity.clone()).filter(|s| !s.is_empty()).unwrap_or("func_detail".into())
    }
}
