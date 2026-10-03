//! The four viewports: 3D perspective + Top/Front/Side orthographic views.

use crate::app::*;
use crate::editor::doc::{Obj, Sel, Xform};
use crate::editor::geom;
use crate::render3d::{self, Batch, Vertex};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use glam::{DQuat, DVec3};
use std::collections::HashMap;

const SEL_COLOR: Color32 = Color32::from_rgb(255, 64, 64);

struct Proj {
    rect: Rect,
    center: (f64, f64),
    zoom: f64,
}

impl Proj {
    fn to_screen(&self, u: f64, v: f64) -> Pos2 {
        Pos2::new(
            self.rect.center().x + ((u - self.center.0) * self.zoom) as f32,
            self.rect.center().y - ((v - self.center.1) * self.zoom) as f32,
        )
    }
    fn to_world(&self, p: Pos2) -> (f64, f64) {
        (
            self.center.0 + (p.x - self.rect.center().x) as f64 / self.zoom,
            self.center.1 - (p.y - self.rect.center().y) as f64 / self.zoom,
        )
    }
}

fn ent_color(e: &crate::formats::vmf::Entity, fgd: &crate::formats::fgd::Fgd) -> Color32 {
    if let Some(c) = e.editor_str("color") {
        if let Some(v) = crate::formats::vmf::parse_vec3(c) {
            return Color32::from_rgb(v.x as u8, v.y as u8, v.z as u8);
        }
    }
    if let Some(c) = fgd.get(e.classname()).and_then(|c| c.color) {
        return Color32::from_rgb(c[0], c[1], c[2]);
    }
    Color32::from_rgb(220, 30, 220)
}

impl App {
    pub fn views_ui(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        let gap = 3.0;
        let rects: [Rect; 4] = if let Some(m) = self.maximized {
            let mut r = [Rect::NOTHING; 4];
            r[m] = full;
            r
        } else {
            let w = (full.width() - gap) / 2.0;
            let h = (full.height() - gap) / 2.0;
            let tl = Rect::from_min_size(full.min, Vec2::new(w, h));
            let tr = Rect::from_min_size(Pos2::new(full.min.x + w + gap, full.min.y), Vec2::new(w, h));
            let bl = Rect::from_min_size(Pos2::new(full.min.x, full.min.y + h + gap), Vec2::new(w, h));
            let br = Rect::from_min_size(Pos2::new(full.min.x + w + gap, full.min.y + h + gap), Vec2::new(w, h));
            [tl, tr, bl, br]
        };
        self.hover_world = None;
        self.prepare_scene();
        for i in 0..4 {
            if rects[i] == Rect::NOTHING {
                continue;
            }
            if i == 0 {
                self.view3d_ui(ui, rects[0]);
            } else {
                self.view2d_ui(ui, rects[i], i - 1);
            }
            // title
            let p = ui.painter_at(rects[i]);
            p.text(
                rects[i].min + Vec2::new(6.0, 4.0),
                egui::Align2::LEFT_TOP,
                VIEW_NAMES[i],
                egui::FontId::proportional(12.0),
                Color32::from_gray(200),
            );
            p.rect_stroke(rects[i], 0.0, Stroke::new(1.0, Color32::from_gray(70)), egui::StrokeKind::Inside);
        }
    }

    // ===================================================================================
    // 2D
    // ===================================================================================

    fn obj_boxes(&self, id: u32) -> Vec<(DVec3, DVec3)> {
        match self.doc.index.get(&id) {
            Some(Obj::WorldSolid(i)) => {
                let s = &self.doc.map.world.solids[*i];
                self.doc.geo.get(&s.id).map(|g| vec![(g.min, g.max)]).unwrap_or_default()
            }
            Some(Obj::Entity(i)) => {
                let e = &self.doc.map.entities[*i];
                if e.solids.is_empty() {
                    vec![self.doc.ent_bounds(e, &self.fgd)]
                } else {
                    e.solids.iter().filter_map(|s| self.doc.geo.get(&s.id)).filter(|g| g.valid).map(|g| (g.min, g.max)).collect()
                }
            }
            None => vec![],
        }
    }

    fn pick_2d(&self, vi: usize, p: (f64, f64), tol: f64) -> Option<u32> {
        let (ua, va, _) = axes(vi);
        let mut best: Option<(f64, u32)> = None;
        for id in self.doc.all_ids() {
            if self.doc.is_hidden(id) {
                continue;
            }
            for (a, b) in self.obj_boxes(id) {
                if p.0 >= a[ua] - tol && p.0 <= b[ua] + tol && p.1 >= a[va] - tol && p.1 <= b[va] + tol {
                    let area = (b[ua] - a[ua]).max(1.0) * (b[va] - a[va]).max(1.0);
                    if best.map(|x| area < x.0).unwrap_or(true) {
                        best = Some((area, id));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    fn drag_xform(&self) -> Option<Xform> {
        match &self.drag {
            Some(Drag::Move { view, start, delta, .. }) => {
                let (ua, va, _) = axes(*view);
                let mut t = DVec3::ZERO;
                let _ = start;
                t[ua] = delta.0;
                t[va] = delta.1;
                Some(Xform::Translate(t))
            }
            Some(Drag::Resize { view, hx, hy, orig, cur }) => {
                let (ua, va, _) = axes(*view);
                let mut origin = (orig.0 + orig.1) * 0.5;
                let mut factor = DVec3::ONE;
                let mut apply = |axis: usize, h: i32, c: f64| {
                    if h == 0 {
                        return;
                    }
                    let (mn, mx) = (orig.0[axis], orig.1[axis]);
                    let len = (mx - mn).max(1e-6);
                    let (anchor, newlen) = if h < 0 { (mx, mx - c) } else { (mn, c - mn) };
                    let newlen = if newlen.abs() < self.grid.min(1.0) { self.grid.min(1.0) } else { newlen };
                    origin[axis] = anchor;
                    factor[axis] = newlen / len;
                };
                apply(ua, *hx, cur.0);
                apply(va, *hy, cur.1);
                Some(Xform::Scale { origin, factor })
            }
            Some(Drag::Rotate { center, angle, view, .. }) => {
                let (_, _, wa) = axes(*view);
                let mut axis = DVec3::ZERO;
                axis[wa] = if *view == 0 { 1.0 } else if *view == 1 { -1.0 } else { 1.0 };
                Some(Xform::Rotate { center: *center, q: DQuat::from_axis_angle(axis, angle.to_radians()) })
            }
            _ => None,
        }
    }

    fn view2d_ui(&mut self, ui: &mut egui::Ui, rect: Rect, vi: usize) {
        let id = egui::Id::new(("view2d", vi));
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(16, 16, 20));
        let (ua, va, wa) = axes(vi);
        let view = self.views[vi];
        let pr = Proj { rect, center: view.center, zoom: view.zoom };
        let hover = resp.hover_pos().filter(|p| rect.contains(*p));
        let mods = ui.input(|i| i.modifiers);

        // ---- zoom / pan ----
        if let Some(h) = hover {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let before = pr.to_world(h);
                let nz = (view.zoom * (1.0 + scroll as f64 * 0.0015)).clamp(0.01, 64.0);
                let v = &mut self.views[vi];
                v.zoom = nz;
                let after = Proj { rect, center: v.center, zoom: nz }.to_world(h);
                v.center.0 += before.0 - after.0;
                v.center.1 += before.1 - after.1;
            }
            let w = pr.to_world(h);
            let mut p = DVec3::ZERO;
            p[ua] = w.0;
            p[va] = w.1;
            self.hover_world = Some(p);
        }
        if resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary) {
            let d = resp.drag_delta();
            let v = &mut self.views[vi];
            v.center.0 -= d.x as f64 / v.zoom;
            v.center.1 += d.y as f64 / v.zoom;
        }
        let view = self.views[vi];
        let pr = Proj { rect, center: view.center, zoom: view.zoom };

        // ---- grid ----
        if self.show_grid {
            let (u0, v1) = pr.to_world(rect.left_top());
            let (u1, v0) = pr.to_world(rect.right_bottom());
            let mut step = self.grid;
            while step * view.zoom < 6.0 {
                step *= 2.0;
            }
            let draw_lines = |painter: &egui::Painter, step: f64, color: Color32, width: f32| {
                let mut x = (u0 / step).floor() * step;
                while x <= u1 {
                    let sx = pr.to_screen(x, 0.0).x;
                    painter.line_segment([Pos2::new(sx, rect.top()), Pos2::new(sx, rect.bottom())], Stroke::new(width, color));
                    x += step;
                }
                let mut y = (v0 / step).floor() * step;
                while y <= v1 {
                    let sy = pr.to_screen(0.0, y).y;
                    painter.line_segment([Pos2::new(rect.left(), sy), Pos2::new(rect.right(), sy)], Stroke::new(width, color));
                    y += step;
                }
            };
            draw_lines(&painter, step, Color32::from_rgb(30, 30, 38), 1.0);
            let mut big = 64.0;
            while big < step {
                big *= 2.0;
            }
            if big * view.zoom >= 8.0 {
                draw_lines(&painter, big, Color32::from_rgb(46, 46, 60), 1.0);
            }
            draw_lines(&painter, 1024.0, Color32::from_rgb(70, 70, 40), 1.0);
            // axes
            let o = pr.to_screen(0.0, 0.0);
            painter.line_segment([Pos2::new(o.x, rect.top()), Pos2::new(o.x, rect.bottom())], Stroke::new(1.0, Color32::from_rgb(40, 90, 40)));
            painter.line_segment([Pos2::new(rect.left(), o.y), Pos2::new(rect.right(), o.y)], Stroke::new(1.0, Color32::from_rgb(90, 40, 40)));
        }

        // ---- objects ----
        let xf = self.drag_xform();
        let vis_u = (pr.to_world(rect.left_top()).0, pr.to_world(rect.right_bottom()).0);
        let vis_v = (pr.to_world(rect.right_bottom()).1, pr.to_world(rect.left_top()).1);
        let in_view = |a: DVec3, b: DVec3| b[ua] >= vis_u.0 && a[ua] <= vis_u.1 && b[va] >= vis_v.0 && a[va] <= vis_v.1;
        let clone_preview = matches!(self.drag, Some(Drag::Move { clone: true, .. }));
        let mut shapes: Vec<egui::Shape> = Vec::new();
        let sel_dim = Color32::from_rgb(190, 190, 200);
        let draw_solid = |shapes: &mut Vec<egui::Shape>, polys: &[Vec<DVec3>], color: Color32, xf: Option<&Xform>| {
            for poly in polys {
                if poly.len() < 2 {
                    continue;
                }
                let pts: Vec<Pos2> = poly
                    .iter()
                    .map(|v| {
                        let v = xf.map(|x| x.point(*v)).unwrap_or(*v);
                        pr.to_screen(v[ua], v[va])
                    })
                    .collect();
                // Faces seen edge-on collapse to a line; stroking them as a closed path makes
                // egui emit huge miter spikes, so skip them and draw plain segments otherwise.
                let mut area = 0.0f32;
                for i in 0..pts.len() {
                    let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                    area += a.x * b.y - b.x * a.y;
                }
                if area.abs() < 1.0 {
                    continue;
                }
                let stroke = Stroke::new(1.0, color);
                for i in 0..pts.len() {
                    shapes.push(egui::Shape::line_segment([pts[i], pts[(i + 1) % pts.len()]], stroke));
                }
            }
        };
        // selected objects go last so they are drawn on top of everything else
        let mut ids: Vec<u32> = self.doc.all_ids().into_iter().collect();
        ids.sort_by_key(|id| self.sel.contains(id));
        let mut flushed = false;
        for id in ids {
            if self.doc.is_hidden(id) {
                continue;
            }
            let selected = self.sel.contains(&id);
            if selected && !flushed {
                painter.extend(std::mem::take(&mut shapes));
                flushed = true;
            }
            let (color, solids_ids, is_point, ent): (Color32, Vec<u32>, bool, Option<&crate::formats::vmf::Entity>) = match self.doc.index.get(&id) {
                Some(Obj::WorldSolid(_)) => (sel_dim, vec![id], false, None),
                Some(Obj::Entity(i)) => {
                    let e = &self.doc.map.entities[*i];
                    (ent_color(e, &self.fgd), e.solids.iter().map(|s| s.id).collect(), e.solids.is_empty(), Some(e))
                }
                None => continue,
            };
            let preview = if selected { xf.as_ref() } else { None };
            if selected && clone_preview {
                // draw the original too
            }
            let color = if selected { SEL_COLOR } else { color };
            if is_point {
                let e = ent.unwrap();
                let (a, b) = self.doc.ent_bounds(e, &self.fgd);
                let (a, b) = match preview {
                    Some(x) => {
                        let (p, q) = (x.point(a), x.point(b));
                        (p.min(q), p.max(q))
                    }
                    None => (a, b),
                };
                if !in_view(a, b) {
                    continue;
                }
                let r = Rect::from_two_pos(pr.to_screen(a[ua], a[va]), pr.to_screen(b[ua], b[va]));
                let r = if r.width() < 4.0 { Rect::from_center_size(r.center(), Vec2::splat(4.0)) } else { r };
                if let Some(ig) = self.inst.get(&id) {
                    // instance: draw its contents, only outline the bounds
                    let line = if selected { SEL_COLOR } else { Color32::from_rgb(120, 130, 160) };
                    if std::env::var("RHAMMER_DEBUG").is_ok() && vi == 0 {
                        for (fi, poly) in ig.polys.iter().enumerate() {
                            let le = (0..poly.len()).map(|i| (poly[i] - poly[(i + 1) % poly.len()]).length()).fold(0.0, f64::max);
                            if le > 2500.0 {
                                eprintln!("INST ent {} file {:?} face {} edge {:.0} mat {} poly {:?}", id, e.get("file"), fi, le, ig.faces[fi].material, poly);
                            }
                        }
                    }
                    draw_solid(&mut shapes, &ig.polys, line, preview);
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, color.linear_multiply(0.6)), egui::StrokeKind::Inside);
                } else {
                    painter.rect_filled(r, 0.0, color.linear_multiply(0.35));
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
                }
                // facing direction
                if e.get("angles").is_some() && !self.inst.contains_key(&id) {
                    let ang = e.angles();
                    let m = crate::editor::doc::angles_matrix(ang);
                    let f = m * DVec3::X;
                    let c = r.center();
                    let len = (r.width().max(r.height()) * 0.8).clamp(10.0, 60.0);
                    let d = Vec2::new(f[ua] as f32, -f[va] as f32);
                    if d.length() > 0.05 {
                        painter.line_segment([c, c + d.normalized() * len], Stroke::new(1.0, color));
                    }
                }
                if self.show_entity_names && view.zoom > 0.35 {
                    let label = e.get("targetname").filter(|t| !t.is_empty()).unwrap_or(e.classname());
                    painter.text(r.right_top() + Vec2::new(3.0, 0.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(10.0), color);
                }
            } else {
                for sid in solids_ids {
                    let Some(g) = self.doc.geo.get(&sid) else { continue };
                    if !g.valid {
                        continue;
                    }
                    let (a, b) = match preview {
                        Some(x) => {
                            let (p, q) = (x.point(g.min), x.point(g.max));
                            (p.min(q), p.max(q))
                        }
                        None => (g.min, g.max),
                    };
                    if !in_view(a, b) {
                        continue;
                    }
                    draw_solid(&mut shapes, &g.polys, color, preview);
                }
            }
        }
        painter.extend(shapes);

        // instance-free: selection box + handles
        let sel_b = self.doc.sel_bounds(&self.sel, &self.fgd);
        let handle_hit = |pos: Pos2, b: (DVec3, DVec3)| -> Option<(i32, i32)> {
            let p0 = pr.to_screen(b.0[ua], b.0[va]);
            let p1 = pr.to_screen(b.1[ua], b.1[va]);
            let (l, r, t, bt) = (p0.x.min(p1.x), p0.x.max(p1.x), p0.y.min(p1.y), p0.y.max(p1.y));
            let cx = (l + r) / 2.0;
            let cy = (t + bt) / 2.0;
            // (hx, hy): -1 = min side in world u/v; +1 = max side
            let xs = [(l, -1), (cx, 0), (r, 1)];
            let ys = [(bt, -1), (cy, 0), (t, 1)];
            for (x, hx) in xs {
                for (y, hy) in ys {
                    if hx == 0 && hy == 0 {
                        continue;
                    }
                    if (pos.x - x).abs() <= 6.0 && (pos.y - y).abs() <= 6.0 {
                        return Some((hx, hy));
                    }
                }
            }
            None
        };
        let draw_handles = |painter: &egui::Painter, b: (DVec3, DVec3), color: Color32, rot: bool| {
            let p0 = pr.to_screen(b.0[ua], b.0[va]);
            let p1 = pr.to_screen(b.1[ua], b.1[va]);
            let r = Rect::from_two_pos(p0, p1);
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, color.linear_multiply(0.6)), egui::StrokeKind::Outside);
            let cx = r.center().x;
            let cy = r.center().y;
            for (x, hx) in [(r.left(), -1), (cx, 0), (r.right(), 1)] {
                for (y, hy) in [(r.bottom(), -1), (cy, 0), (r.top(), 1)] {
                    if hx == 0 && hy == 0 {
                        continue;
                    }
                    if rot {
                        // rotation mode: round handles on the corners only
                        if hx != 0 && hy != 0 {
                            painter.circle_filled(Pos2::new(x, y), 5.0, color);
                        }
                    } else {
                        painter.rect_filled(Rect::from_center_size(Pos2::new(x, y), Vec2::splat(7.0)), 0.0, color);
                    }
                }
            }
            if rot {
                painter.circle_stroke(r.center(), 3.0, Stroke::new(1.0, color));
            }
            // Hammer-style dimensions: width above the box, height left of it
            let s = b.1 - b.0;
            let fmt = |v: f64| {
                let v = v.abs();
                if (v - v.round()).abs() < 0.005 { format!("{:.0}", v) } else { format!("{:.2}", v) }
            };
            let font = egui::FontId::monospace(11.0);
            painter.text(r.center_top() - Vec2::new(0.0, 8.0), egui::Align2::CENTER_BOTTOM, fmt(s[ua]), font.clone(), color);
            painter.text(r.left_center() - Vec2::new(8.0, 0.0), egui::Align2::RIGHT_CENTER, fmt(s[va]), font, color);
        };

        if self.tool == Tool::Select {
            if let Some(b) = sel_b {
                let b = match &xf {
                    Some(Xform::Rotate { .. }) | None => b,
                    Some(x) => {
                        let (p, q) = (x.point(b.0), x.point(b.1));
                        (p.min(q), p.max(q))
                    }
                };
                draw_handles(&painter, b, Color32::from_rgb(255, 220, 80), self.rot_mode);
            }
        }
        if self.tool == Tool::Block {
            if let Some(b) = self.block {
                let r = Rect::from_two_pos(pr.to_screen(b.0[ua], b.0[va]), pr.to_screen(b.1[ua], b.1[va]));
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(255, 255, 0, 20));
                draw_handles(&painter, b, Color32::YELLOW, false);
            }
        }
        if let Some(Drag::BoxSel { view: v, start, cur }) = &self.drag {
            if *v == vi {
                let r = Rect::from_two_pos(pr.to_screen(start.0, start.1), pr.to_screen(cur.0, cur.1));
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(120, 160, 255, 30));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(120, 160, 255)), egui::StrokeKind::Inside);
            }
        }
        if self.tool == Tool::Clip && self.clip.view == vi {
            if let (Some(p0), Some(p1)) = (self.clip.p0, self.clip.p1) {
                let a = pr.to_screen(p0.0, p0.1);
                let b = pr.to_screen(p1.0, p1.1);
                let dir = (b - a).normalized();
                if dir.length() > 0.0 {
                    painter.line_segment([a - dir * 2000.0, b + dir * 2000.0], Stroke::new(1.0, Color32::from_rgb(255, 120, 0)));
                    // front side indicator
                    let nrm = Vec2::new(dir.y, -dir.x);
                    let mid = a + (b - a) * 0.5;
                    painter.line_segment([mid, mid + nrm * 18.0], Stroke::new(2.0, Color32::from_rgb(255, 200, 0)));
                }
                painter.circle_filled(a, 4.0, Color32::YELLOW);
                painter.circle_filled(b, 4.0, Color32::YELLOW);
            }
        }

        // cursor feedback
        if let Some(h) = hover {
            if self.tool == Tool::Select {
                if let Some(b) = sel_b {
                    if let Some((hx, hy)) = handle_hit(h, b) {
                        ui.ctx().set_cursor_icon(match (hx, hy) {
                            (0, _) => egui::CursorIcon::ResizeVertical,
                            (_, 0) => egui::CursorIcon::ResizeHorizontal,
                            (a, b) if a * b > 0 => egui::CursorIcon::ResizeNeSw,
                            _ => egui::CursorIcon::ResizeNwSe,
                        });
                    }
                }
            }
        }

        // ---- interaction ----
        let wpos = |p: Pos2| pr.to_world(p);
        let snap2 = |s: &App, p: (f64, f64)| (s.snapv(p.0), s.snapv(p.1));
        let ctrl = mods.command;
        let press_origin = ui.input(|i| i.pointer.press_origin());

        if resp.drag_started_by(egui::PointerButton::Primary) {
            if let Some(po) = press_origin.filter(|p| rect.contains(*p)) {
                let w = wpos(po);
                match self.tool {
                    Tool::Select | Tool::Texture => {
                        let sb = sel_b;
                        if let (Some(b), true) = (sb, self.tool == Tool::Select) {
                            if let Some((hx, hy)) = handle_hit(po, b).filter(|(hx, hy)| !self.rot_mode || (*hx != 0 && *hy != 0)) {
                                if self.rot_mode {
                                    let c = (b.0 + b.1) * 0.5;
                                    let start_angle = (w.1 - c[va]).atan2(w.0 - c[ua]);
                                    self.drag = Some(Drag::Rotate { view: vi, center: c, start_angle, angle: 0.0 });
                                } else {
                                    self.drag = Some(Drag::Resize { view: vi, hx, hy, orig: b, cur: snap2(self, w) });
                                }
                            } else if w.0 >= b.0[ua] && w.0 <= b.1[ua] && w.1 >= b.0[va] && w.1 <= b.1[va] && !ctrl {
                                self.drag = Some(Drag::Move { view: vi, start: snap2(self, w), delta: (0.0, 0.0), clone: mods.shift });
                            } else if let Some(id) = self.pick_2d(vi, w, 3.0 / view.zoom).filter(|_| !ctrl) {
                                if !self.sel.contains(&id) {
                                    self.set_sel([id].into_iter().collect());
                                }
                                self.drag = Some(Drag::Move { view: vi, start: snap2(self, w), delta: (0.0, 0.0), clone: mods.shift });
                            } else {
                                self.drag = Some(Drag::BoxSel { view: vi, start: w, cur: w });
                            }
                        } else if self.tool == Tool::Select {
                            if let Some(id) = self.pick_2d(vi, w, 3.0 / view.zoom).filter(|_| !ctrl) {
                                self.set_sel([id].into_iter().collect());
                                self.drag = Some(Drag::Move { view: vi, start: snap2(self, w), delta: (0.0, 0.0), clone: mods.shift });
                            } else {
                                self.drag = Some(Drag::BoxSel { view: vi, start: w, cur: w });
                            }
                        }
                    }
                    Tool::Block => {
                        let sw = snap2(self, w);
                        let mut handled = false;
                        if let Some(b) = self.block {
                            if let Some((hx, hy)) = handle_hit(po, b) {
                                self.drag = Some(Drag::BlockResize { view: vi, hx, hy, orig: b });
                                handled = true;
                            } else if w.0 >= b.0[ua].min(b.1[ua]) && w.0 <= b.0[ua].max(b.1[ua]) && w.1 >= b.0[va].min(b.1[va]) && w.1 <= b.0[va].max(b.1[va]) {
                                self.drag = Some(Drag::BlockMove { view: vi, start: sw, orig: b });
                                handled = true;
                            }
                        }
                        if !handled {
                            let (mut a, mut b) = self.block.unwrap_or_else(|| {
                                let mut a = DVec3::ZERO;
                                let mut b = DVec3::ZERO;
                                a[wa] = 0.0;
                                b[wa] = 64.0f64.max(self.grid);
                                (a, b)
                            });
                            if self.block.is_none() {
                                a[wa] = 0.0;
                                b[wa] = 64.0f64.max(self.grid);
                            }
                            a[ua] = sw.0;
                            a[va] = sw.1;
                            b[ua] = sw.0;
                            b[va] = sw.1;
                            self.block = Some((a, b));
                            self.drag = Some(Drag::BlockNew { view: vi, start: sw });
                        }
                    }
                    Tool::Clip => {
                        let sw = snap2(self, w);
                        self.clip.view = vi;
                        self.clip.p0 = Some(sw);
                        self.clip.p1 = Some(sw);
                        self.drag = Some(Drag::ClipLine { view: vi });
                    }
                    Tool::Entity => {}
                }
            }
        }

        if resp.dragged_by(egui::PointerButton::Primary) {
            if let Some(cp) = resp.interact_pointer_pos() {
                let w = wpos(cp);
                let sw = snap2(self, w);
                let grid = self.grid;
                let snap = self.snap;
                let sn = |v: f64| if snap { (v / grid).round() * grid } else { v };
                let mut block_upd: Option<(DVec3, DVec3)> = None;
                match &mut self.drag {
                    Some(Drag::Move { view: v, start, delta, .. }) if *v == vi => {
                        *delta = (sw.0 - start.0, sw.1 - start.1);
                    }
                    Some(Drag::Resize { view: v, cur, .. }) if *v == vi => *cur = sw,
                    Some(Drag::Rotate { view: v, center, start_angle, angle }) if *v == vi => {
                        let a = (w.1 - center[va]).atan2(w.0 - center[ua]);
                        let mut deg = (a - *start_angle).to_degrees();
                        if snap {
                            deg = (deg / 15.0).round() * 15.0;
                        }
                        *angle = deg;
                    }
                    Some(Drag::BoxSel { view: v, cur, .. }) if *v == vi => *cur = w,
                    Some(Drag::ClipLine { view: v }) if *v == vi => {
                        self.clip.p1 = Some(sw);
                    }
                    Some(Drag::BlockNew { view: v, start }) if *v == vi => {
                        if let Some((a, b)) = self.block.as_mut() {
                            a[ua] = start.0;
                            a[va] = start.1;
                            b[ua] = sw.0;
                            b[va] = sw.1;
                        }
                    }
                    Some(Drag::BlockMove { view: v, start, orig }) if *v == vi => {
                        let d = (sw.0 - start.0, sw.1 - start.1);
                        let (mut a, mut b) = *orig;
                        a[ua] += d.0;
                        b[ua] += d.0;
                        a[va] += d.1;
                        b[va] += d.1;
                        block_upd = Some((a, b));
                    }
                    Some(Drag::BlockResize { view: v, hx, hy, orig }) if *v == vi => {
                        let (mut a, mut b) = *orig;
                        // normalise so a <= b on both axes
                        for ax in [ua, va] {
                            if a[ax] > b[ax] {
                                std::mem::swap(&mut a[ax], &mut b[ax]);
                            }
                        }
                        match hx.signum() {
                            -1 => a[ua] = sn(w.0),
                            1 => b[ua] = sn(w.0),
                            _ => {}
                        }
                        match hy.signum() {
                            -1 => a[va] = sn(w.1),
                            1 => b[va] = sn(w.1),
                            _ => {}
                        }
                        block_upd = Some((a, b));
                    }
                    _ => {}
                }
                if let Some(b) = block_upd {
                    self.block = Some(b);
                }
            }
        }

        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            match self.drag.take() {
                Some(Drag::Move { delta, clone, view, .. }) => {
                    if delta != (0.0, 0.0) || clone {
                        let (ua, va, _) = axes(view);
                        let mut t = DVec3::ZERO;
                        t[ua] = delta.0;
                        t[va] = delta.1;
                        self.doc.checkpoint();
                        if clone {
                            let s = self.sel.clone();
                            let n = self.doc.clone_objects(&s, DVec3::ZERO);
                            self.set_sel(n);
                        }
                        let s = self.sel.clone();
                        self.doc.transform(&s, &Xform::Translate(t), self.tex_lock);
                    }
                }
                Some(d @ (Drag::Resize { .. } | Drag::Rotate { .. })) => {
                    self.drag = Some(d);
                    if let Some(x) = self.drag_xform() {
                        self.drag = None;
                        self.doc.checkpoint();
                        let s = self.sel.clone();
                        self.doc.transform(&s, &x, self.tex_lock);
                    }
                }
                Some(Drag::BoxSel { view, start, cur }) => {
                    let (ua, va, _) = axes(view);
                    let (u0, u1) = (start.0.min(cur.0), start.0.max(cur.0));
                    let (v0, v1) = (start.1.min(cur.1), start.1.max(cur.1));
                    let mut found = Sel::new();
                    if (u1 - u0) * view_zoom(&self.views[view]) > 3.0 || (v1 - v0) * view_zoom(&self.views[view]) > 3.0 {
                        for id in self.doc.all_ids() {
                            if self.doc.is_hidden(id) {
                                continue;
                            }
                            let boxes = self.obj_boxes(id);
                            if !boxes.is_empty() && boxes.iter().all(|(a, b)| a[ua] >= u0 && b[ua] <= u1 && a[va] >= v0 && b[va] <= v1) {
                                found.insert(id);
                            }
                        }
                        if mods.shift || ctrl {
                            let mut s = self.sel.clone();
                            for f in found {
                                if !s.remove(&f) {
                                    s.insert(f);
                                }
                            }
                            self.set_sel(s);
                        } else {
                            self.set_sel(found);
                        }
                    }
                }
                _ => {}
            }
        }

        if resp.clicked_by(egui::PointerButton::Primary) {
            if let Some(cp) = resp.interact_pointer_pos() {
                let w = wpos(cp);
                match self.tool {
                    Tool::Select | Tool::Texture => {
                        let hit = self.pick_2d(vi, w, 3.0 / view.zoom);
                        match hit {
                            Some(id) => {
                                if ctrl || mods.shift {
                                    let mut s = self.sel.clone();
                                    if !s.remove(&id) {
                                        s.insert(id);
                                    }
                                    self.set_sel(s);
                                    self.rot_mode = false;
                                } else if self.sel.contains(&id) && self.tool == Tool::Select {
                                    // clicking an already selected object toggles rotation handles
                                    self.rot_mode = !self.rot_mode;
                                } else {
                                    self.set_sel([id].into_iter().collect());
                                    self.rot_mode = false;
                                }
                            }
                            None => {
                                if !(ctrl || mods.shift) {
                                    self.set_sel(Sel::new());
                                }
                                self.rot_mode = false;
                            }
                        }
                    }
                    Tool::Entity => {
                        let mut p = DVec3::ZERO;
                        p[ua] = self.snapv(w.0);
                        p[va] = self.snapv(w.1);
                        p[wa] = 0.0;
                        self.place_entity(p);
                    }
                    _ => {}
                }
            }
        }

        // arrow-key nudge
        if hover.is_some() && !ui.ctx().egui_wants_keyboard_input() && self.tool == Tool::Select && !self.sel.is_empty() {
            let mut d = (0.0, 0.0);
            ui.input(|i| {
                if i.key_pressed(egui::Key::ArrowLeft) { d.0 -= 1.0; }
                if i.key_pressed(egui::Key::ArrowRight) { d.0 += 1.0; }
                if i.key_pressed(egui::Key::ArrowUp) { d.1 += 1.0; }
                if i.key_pressed(egui::Key::ArrowDown) { d.1 -= 1.0; }
            });
            if d != (0.0, 0.0) {
                let mut t = DVec3::ZERO;
                t[ua] = d.0 * self.grid;
                t[va] = d.1 * self.grid;
                self.apply_xform(Xform::Translate(t));
            }
        }
        // maximize toggle
        if hover.is_some() && !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Z) && !i.modifiers.command) {
            self.maximized = if self.maximized.is_some() { None } else { Some(vi + 1) };
        }
    }

    pub fn place_entity(&mut self, p: DVec3) {
        self.doc.checkpoint();
        let class = self.ent_class.clone();
        let id = self.doc.create_entity(&class, p, &self.fgd);
        self.set_sel([id].into_iter().collect());
        self.tab = RightTab::Object;
        self.status = format!("Created {class}");
    }

    // ===================================================================================
    // 3D
    // ===================================================================================

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
    fn prepare_scene(&mut self) {
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
        if let Ok(mut sh) = self.shared.lock() {
            sh.scene.uploads.append(&mut self.mats.ready);
        }
    }

    /// Load (a few per frame) the models referenced by entities and instances, and publish their hulls.
    fn ensure_models(&mut self) {
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
                let m = crate::assets::mdl::load(&self.mats.fs, &p).map(std::rc::Rc::new);
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

    fn rebuild_instances(&mut self) {
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

    fn rebuild_world(&mut self) {
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
                point_boxes.push((a, b, [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0]));
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
    fn rebuild_models(&mut self) {
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

    fn rebuild_overlay(&mut self) {
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
            for (sd, poly) in s.sides.iter().zip(&g.polys) {
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
                if let Some(t) = geom::ray_aabb(o, d, a, b) {
                    consider(e.id, None, t);
                }
            } else {
                for s in &e.solids {
                    let Some(g) = self.doc.geo.get(&s.id) else { continue };
                    if geom::ray_aabb(o, d, g.min - DVec3::splat(1.0), g.max + DVec3::splat(1.0)).is_none() {
                        continue;
                    }
                    for (sd, poly) in s.sides.iter().zip(&g.polys) {
                        if let Some(t) = geom::ray_poly(o, d, poly) {
                            consider(e.id, Some(sd.id), t);
                        }
                    }
                }
            }
        }
        best
    }

    fn view3d_ui(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let id = egui::Id::new("view3d");
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
        if let Some(h) = resp.hover_pos().filter(|_| hovered) {
            let (o, d) = ray_at(self, h);
            if let Some((_, _, t)) = self.pick_3d(o, d) {
                self.hover_world = Some(o + d * t);
            }
        }
        if resp.clicked_by(egui::PointerButton::Primary) {
            if let Some(p) = resp.interact_pointer_pos() {
                let (o, d) = ray_at(self, p);
                let hit = self.pick_3d(o, d);
                match self.tool {
                    Tool::Select | Tool::Block | Tool::Clip => match hit {
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
        if hovered && ui.input(|i| i.key_pressed(egui::Key::Z) && !i.modifiers.command) && !ui.ctx().egui_wants_keyboard_input() {
            self.maximized = if self.maximized.is_some() { None } else { Some(0) };
        }
    }
}

fn view_zoom(v: &View2D) -> f64 {
    v.zoom
}
