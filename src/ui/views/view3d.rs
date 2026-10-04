//! The 3D perspective viewport.

use crate::app::*;
use crate::editor::doc::Sel;
use crate::editor::geom;
use crate::render3d::{self};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke};
use glam::DVec3;

/// True if the ray hits the polygon from behind (convex solid, normal oriented away from `center`),
/// so a camera inside a brush clicks through its faces.
fn back_facing(d: DVec3, center: DVec3, poly: &[DVec3]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut n = (poly[1] - poly[0]).cross(poly[2] - poly[0]);
    if n.dot(poly[0] - center) < 0.0 {
        n = -n;
    }
    n.dot(d) > 0.0
}

impl App {
    /// Ray pick: returns (object id, side id (if solid), distance).
    pub fn pick_3d(&self, o: DVec3, d: DVec3) -> Option<(u32, Option<u32>, f64)> {
        let mut best: Option<(u32, Option<u32>, f64)> = None;
        let mut consider = |id: u32, side: Option<u32>, t: f64| {
            if best.map(|b| t < b.2).unwrap_or(true) {
                best = Some((id, side, t));
            }
        };
        for s in &self.doc.map.world.solids {
            if self.doc.is_hidden(s.id) {
                continue;
            }
            let Some(g) = self.doc.geo.get(&s.id) else { continue };
            if geom::ray_aabb(o, d, g.min - DVec3::splat(1.0), g.max + DVec3::splat(1.0)).is_none() {
                continue;
            }
            let center = (g.min + g.max) * 0.5;
            for (sd, poly) in s.sides.iter().zip(&g.polys) {
                if back_facing(d, center, poly) {
                    continue;
                }
                if let Some(t) = geom::ray_poly(o, d, poly) {
                    consider(s.id, Some(sd.id), t);
                }
            }
        }
        for e in &self.doc.map.entities {
            if self.doc.is_hidden(e.id) {
                continue;
            }
            if e.solids.is_empty() {
                if let Some(ig) = self.inst.get(&e.id) {
                    if geom::ray_aabb(o, d, ig.min, ig.max).is_some() {
                        for poly in &ig.polys {
                            if let Some(t) = geom::ray_poly(o, d, poly) {
                                consider(e.id, None, t);
                            }
                        }
                    }
                    continue;
                }
                let (a, b) = self.doc.ent_bounds(e, &self.fgd);
                // camera inside the box: ignore it so clicks pass through
                let inside = (0..3).all(|i| o[i] > a[i] && o[i] < b[i]);
                if inside {
                    continue;
                }
                if let Some(t) = geom::ray_aabb(o, d, a, b) {
                    consider(e.id, None, t);
                }
            } else {
                for s in &e.solids {
                    let Some(g) = self.doc.geo.get(&s.id) else { continue };
                    if geom::ray_aabb(o, d, g.min - DVec3::splat(1.0), g.max + DVec3::splat(1.0)).is_none() {
                        continue;
                    }
                    let center = (g.min + g.max) * 0.5;
                    for (sd, poly) in s.sides.iter().zip(&g.polys) {
                        if back_facing(d, center, poly) {
                            continue;
                        }
                        if let Some(t) = geom::ray_poly(o, d, poly) {
                            consider(e.id, Some(sd.id), t);
                        }
                    }
                }
            }
        }
        best
    }

    pub(crate) fn view3d_ui(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let id = ui.id().with("view3d");
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        let hovered = resp.hovered();
        let aspect = rect.width() / rect.height().max(1.0);

        // ---- camera control ----
        let rmb = ui.input(|i| i.pointer.button_down(egui::PointerButton::Secondary));
        let looking = resp.dragged_by(egui::PointerButton::Secondary) || (rmb && hovered);
        if looking {
            let d = ui.input(|i| i.pointer.delta());
            self.cam.yaw -= d.x as f64 * 0.25;
            self.cam.pitch = (self.cam.pitch - d.y as f64 * 0.25).clamp(-89.0, 89.0);
        }
        if resp.dragged_by(egui::PointerButton::Middle) {
            let d = resp.drag_delta();
            let r = self.cam.right();
            self.cam.pos -= r * d.x as f64 * 1.5;
            self.cam.pos += DVec3::Z * d.y as f64 * 1.5;
        }
        if hovered || looking {
            let dt = ui.input(|i| i.stable_dt).min(0.1) as f64;
            let speed = if ui.input(|i| i.modifiers.shift) { 1600.0 } else { 500.0 };
            let (mut f, mut r, mut up) = (0.0, 0.0, 0.0);
            // letters only fly while RMB is held (otherwise they are tool hotkeys); arrows always work
            ui.input(|i| {
                let k = |key: egui::Key| rmb && i.key_down(key);
                if k(egui::Key::W) || i.key_down(egui::Key::ArrowUp) { f += 1.0; }
                if k(egui::Key::S) || i.key_down(egui::Key::ArrowDown) { f -= 1.0; }
                if k(egui::Key::D) || i.key_down(egui::Key::ArrowRight) { r += 1.0; }
                if k(egui::Key::A) || i.key_down(egui::Key::ArrowLeft) { r -= 1.0; }
                if k(egui::Key::E) || k(egui::Key::Space) { up += 1.0; }
                if k(egui::Key::Q) || k(egui::Key::C) { up -= 1.0; }
            });
            let typing = ui.ctx().egui_wants_keyboard_input() || ui.input(|i| i.modifiers.command);
            if !typing && (f != 0.0 || r != 0.0 || up != 0.0) {
                let fwd = self.cam.forward();
                let rt = self.cam.right();
                self.cam.pos += (fwd * f + rt * r + DVec3::Z * up) * speed * dt;
                ui.ctx().request_repaint();
            }
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.cam.pos += self.cam.forward() * scroll as f64 * 2.0;
            }
            if looking {
                ui.ctx().request_repaint();
            }
        }

        // ---- picking ----
        let ctrl = ui.input(|i| i.modifiers.command);
        let shift = ui.input(|i| i.modifiers.shift);
        let ray_at = |app: &App, p: Pos2| {
            let ndc = (((p.x - rect.left()) / rect.width()) * 2.0 - 1.0, 1.0 - ((p.y - rect.top()) / rect.height()) * 2.0);
            app.cam.ray(aspect, ndc)
        };
        let cam = self.cam.clone();
        let project = |p: DVec3| {
            cam.project(aspect, p).map(|p| Pos2::new(rect.left() + (p.0 * 0.5 + 0.5) * rect.width(), rect.top() + (0.5 - p.1 * 0.5) * rect.height()))
        };
        // vertex tool: drag on the world plane most facing the camera, hit by the mouse ray
        let plane_hit = |o: DVec3, d: DVec3, n: usize, at: f64| (d[n].abs() > 1e-6).then(|| o + d * ((at - o[n]) / d[n])).filter(|_| (at - o[n]) / d[n] > 0.0);
        if self.tool == Tool::Vertex && resp.drag_started_by(egui::PointerButton::Primary) {
            if let Some(po) = ui.input(|i| i.pointer.press_origin()).filter(|p| rect.contains(*p)) {
                let hit = self.vertex_hit(&project, po);
                self.vertex_select(hit.clone(), ctrl || shift);
                if let Some((_, grab)) = hit.into_iter().find(|(id, p)| self.vtx.sel.contains(&(*id, *p))) {
                    let f = cam.forward().abs();
                    let n = if f.x >= f.y && f.x >= f.z { 0 } else if f.y >= f.z { 1 } else { 2 };
                    let (o, d) = ray_at(self, po);
                    if let Some(start) = plane_hit(o, d, n, grab[n]) {
                        let plane = match n { 0 => [1, 2], 1 => [0, 2], _ => [0, 1] };
                        self.drag = Some(Drag::Vertex { view: 3, plane, start, grab, delta: DVec3::ZERO });
                    }
                }
            }
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            if let (Some(Drag::Vertex { view: 3, plane, start, grab, .. }), Some(p)) = (&self.drag, resp.interact_pointer_pos()) {
                let n = 3 - plane[0] - plane[1];
                let (o, d) = ray_at(self, p);
                if let Some(cur) = plane_hit(o, d, n, start[n]) {
                    let nd = self.vertex_delta(*grab, *start, cur, *plane);
                    if let Some(Drag::Vertex { delta, .. }) = &mut self.drag {
                        *delta = nd;
                    }
                }
            }
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            if let Some(Drag::Vertex { view: 3, delta, .. }) = self.drag {
                self.drag = None;
                self.vertex_commit(delta);
            }
        }
        if let Some(h) = resp.hover_pos().filter(|_| hovered) {
            let (o, d) = ray_at(self, h);
            if let Some((_, _, t)) = self.pick_3d(o, d) {
                self.hover_world = Some(o + d * t);
            }
        }
        if resp.clicked_by(egui::PointerButton::Primary) {
            if let Some(p) = resp.interact_pointer_pos() {
                let (o, d) = ray_at(self, p);
                'click: {
                let hit = self.pick_3d(o, d);
                if self.tool == Tool::Vertex {
                    let corners = self.vertex_hit(&project, p);
                    if !corners.is_empty() {
                        self.vertex_select(corners, ctrl || shift);
                        break 'click;
                    }
                }
                match self.tool {
                    Tool::Select | Tool::Block | Tool::Clip | Tool::Vertex => match hit {
                        Some((id, _, _)) => {
                            if ctrl || shift {
                                let mut s = self.sel.clone();
                                if !s.remove(&id) {
                                    s.insert(id);
                                }
                                self.set_sel(s);
                            } else {
                                self.set_sel([id].into_iter().collect());
                            }
                        }
                        None => {
                            if !(ctrl || shift) {
                                self.set_sel(Sel::new());
                            }
                        }
                    },
                    Tool::Entity => {
                        let pt = match hit {
                            Some((_, _, t)) => o + d * t,
                            None => o + d * 256.0,
                        };
                        // lift entities off the surface slightly so they are not buried
                        let pt = DVec3::new(self.snapv(pt.x), self.snapv(pt.y), self.snapv(pt.z));
                        self.place_entity(pt);
                    }
                    Tool::Texture => {
                        if let Some((_, Some(side), _)) = hit {
                            if ctrl || shift {
                                if !self.faces.remove(&side) {
                                    self.faces.insert(side);
                                }
                            } else {
                                self.faces.clear();
                                self.faces.insert(side);
                            }
                            self.load_face_edit();
                            self.bump_sel();
                        } else if !(ctrl || shift) {
                            self.faces.clear();
                            self.bump_sel();
                        }
                    }
                }
                }
            }
        }
        if resp.clicked_by(egui::PointerButton::Secondary) && self.tool == Tool::Texture {
            if let Some(p) = resp.interact_pointer_pos() {
                let (o, d) = ray_at(self, p);
                if let Some((_, Some(side), _)) = self.pick_3d(o, d) {
                    let mut f = self.faces.clone();
                    if !f.contains(&side) {
                        f.clear();
                        f.insert(side);
                    }
                    self.faces = f;
                    self.apply_material_to_faces();
                }
            }
        }

        // ---- draw ----
        if let Ok(mut sh) = self.shared.lock() {
            sh.scene.mvp = self.cam.matrices(aspect);
            sh.scene.lighting = true;
            sh.scene.cull = true;
            sh.scene.wireframe = self.wireframe;
            sh.scene.clear = [0.1, 0.11, 0.14];
        }
        ui.painter().add(render3d::paint_callback(self.shared.clone(), rect));

        // draw the block preview as 2D overlay lines in 3D
        if self.tool == Tool::Block {
            if let Some((a, b)) = self.block {
                let c = |i: usize| DVec3::new(if i & 4 != 0 { b.x } else { a.x }, if i & 2 != 0 { b.y } else { a.y }, if i & 1 != 0 { b.z } else { a.z });
                let painter = ui.painter_at(rect);
                for (i, j) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                    if let (Some(p), Some(q)) = (self.cam.project(aspect, c(i)), self.cam.project(aspect, c(j))) {
                        let s = |p: (f32, f32)| Pos2::new(rect.left() + (p.0 * 0.5 + 0.5) * rect.width(), rect.top() + (0.5 - p.1 * 0.5) * rect.height());
                        painter.line_segment([s(p), s(q)], Stroke::new(1.5, Color32::YELLOW));
                    }
                }
            }
        }
        if self.tool == Tool::Vertex {
            let delta = match &self.drag {
                Some(Drag::Vertex { view: 3, delta, .. }) => Some(*delta),
                _ => None,
            };
            self.vertex_draw(&ui.painter_at(rect), &project, delta);
        }
        if hovered && ui.input(|i| i.key_pressed(egui::Key::Z) && !i.modifiers.command) && !ui.ctx().egui_wants_keyboard_input() {
            self.maximized = if self.maximized.is_some() { None } else { Some(0) };
        }
    }
}
