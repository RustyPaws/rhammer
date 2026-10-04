//! 2D orthographic viewports: drawing, picking and mouse interaction.

use crate::app::*;
use crate::editor::doc::{Obj, Sel, Xform};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use glam::{DQuat, DVec3};
use super::*;
use crate::editor::direction::entity_direction;

impl App {
    pub(crate) fn obj_boxes(&self, id: u32) -> Vec<(DVec3, DVec3)> {
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

    pub(crate) fn pick_2d(&self, vi: usize, p: (f64, f64), tol: f64) -> Option<u32> {
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

    pub(crate) fn drag_xform(&self) -> Option<Xform> {
        match &self.drag {
            Some(Drag::Move { view, delta, .. }) => {
                let (ua, va, _) = axes(*view);
                let mut t = DVec3::ZERO;
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

    pub(crate) fn view2d_ui(&mut self, ui: &mut egui::Ui, rect: Rect, vi: usize) {
        let id = ui.id().with(("view2d", vi));
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
                if selected && !self.inst.contains_key(&id) {
                    if let Some(f) = entity_direction(e, &self.fgd) {
                        let len = (r.width().max(r.height()) * 0.8).clamp(14.0, 60.0);
                        overlays::draw_direction(&painter, r.center(), Vec2::new(f[ua] as f32, -f[va] as f32), len, color);
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
                // brush entity direction (door move direction, push direction …)
                if let Some((e, f)) = ent.filter(|_| selected).and_then(|e| entity_direction(e, &self.fgd).map(|f| (e, f))) {
                    let (a, b) = self.doc.ent_bounds(e, &self.fgd);
                    let (a, b) = match preview {
                        Some(x) => {
                            let (p, q) = (x.point(a), x.point(b));
                            (p.min(q), p.max(q))
                        }
                        None => (a, b),
                    };
                    if in_view(a, b) {
                        let size = b - a;
                        let extent = (f.x * size.x).abs() + (f.y * size.y).abs() + (f.z * size.z).abs();
                        let len = ((extent * 0.5 * view.zoom) as f32).clamp(18.0, 160.0);
                        let c = pr.to_screen((a[ua] + b[ua]) * 0.5, (a[va] + b[va]) * 0.5);
                        overlays::draw_direction(&painter, c, Vec2::new(f[ua] as f32, -f[va] as f32), len, color);
                    }
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

        // origin marker for the single selected brush entity (drag it to set `origin`)
        let origin_marker: Option<(u32, DVec3)> = (self.tool == Tool::Select && self.sel.len() == 1)
            .then(|| self.sel.iter().next().copied())
            .flatten()
            .and_then(|id| self.doc.entity(id).filter(|e| !e.solids.is_empty()).map(|e| (id, e)))
            .map(|(id, e)| {
                let (a, b) = self.doc.ent_bounds(e, &self.fgd);
                (id, if e.get("origin").is_some() { e.origin() } else { (a + b) * 0.5 })
            });
        if let Some((id, o)) = origin_marker {
            let mut o = o;
            if let Some(Drag::Origin { view: v, id: did, cur, .. }) = &self.drag {
                if *v == vi && *did == id {
                    o[ua] = cur.0;
                    o[va] = cur.1;
                }
            }
            overlays::draw_origin_marker(&painter, pr.to_screen(o[ua], o[va]), Color32::from_rgb(80, 220, 255));
        }

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
        let project = |p: DVec3| Some(pr.to_screen(p[ua], p[va]));
        if self.tool == Tool::Vertex {
            let delta = match &self.drag {
                Some(Drag::Vertex { view: v, delta, .. }) if *v == vi => Some(*delta),
                _ => None,
            };
            self.vertex_draw(&painter, &project, delta);
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
                        let hit_origin = origin_marker.filter(|(_, o)| {
                            let sp = pr.to_screen(o[ua], o[va]);
                            self.tool == Tool::Select && (sp - po).length() <= 8.0
                        });
                        if let Some((id, o)) = hit_origin {
                            self.drag = Some(Drag::Origin { view: vi, id, orig: o, cur: (o[ua], o[va]) });
                        } else
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
                    Tool::Vertex => {
                        let hit = self.vertex_hit(&project, po);
                        if hit.is_empty() {
                            self.drag = Some(Drag::BoxSel { view: vi, start: w, cur: w });
                        } else {
                            self.vertex_select(hit.clone(), mods.shift || ctrl);
                            if let Some((_, grab)) = hit.into_iter().find(|(id, p)| self.vtx.sel.contains(&(*id, *p))) {
                                let mut start = DVec3::ZERO;
                                start[ua] = w.0;
                                start[va] = w.1;
                                self.drag = Some(Drag::Vertex { view: vi, plane: [ua, va], start, grab, delta: DVec3::ZERO });
                            }
                        }
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
                if let Some(Drag::Vertex { view: v, plane, start, grab, .. }) = &self.drag {
                    if *v == vi {
                        let mut cur = *start;
                        cur[ua] = w.0;
                        cur[va] = w.1;
                        let d = self.vertex_delta(*grab, *start, cur, *plane);
                        if let Some(Drag::Vertex { delta, .. }) = &mut self.drag {
                            *delta = d;
                        }
                    }
                }
                match &mut self.drag {
                    Some(Drag::Move { view: v, start, delta, .. }) if *v == vi => {
                        *delta = (sw.0 - start.0, sw.1 - start.1);
                    }
                    Some(Drag::Resize { view: v, cur, .. }) if *v == vi => *cur = sw,
                    Some(Drag::Origin { view: v, cur, .. }) if *v == vi => *cur = sw,
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
                Some(Drag::Vertex { delta, .. }) => self.vertex_commit(delta),
                Some(Drag::Origin { view, id, orig, cur }) => {
                    let (ua, va, _) = axes(view);
                    let mut o = orig;
                    o[ua] = cur.0;
                    o[va] = cur.1;
                    if o != orig {
                        self.doc.checkpoint();
                        self.doc.set_entity_origin(id, o);
                    }
                }
                Some(Drag::BoxSel { view, start, cur }) => {
                    let (ua, va, _) = axes(view);
                    let (u0, u1) = (start.0.min(cur.0), start.0.max(cur.0));
                    let (v0, v1) = (start.1.min(cur.1), start.1.max(cur.1));
                    let mut found = Sel::new();
                    if self.tool == Tool::Vertex && !self.sel.is_empty() {
                        let area = Rect::from_two_pos(pr.to_screen(u0, v0), pr.to_screen(u1, v1));
                        self.vertex_box_select(&project, area, mods.shift || ctrl);
                    } else if (u1 - u0) * view_zoom(&self.views[view]) > 3.0 || (v1 - v0) * view_zoom(&self.views[view]) > 3.0 {
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
                    Tool::Vertex if !self.vertex_hit(&project, cp).is_empty() => {
                        let hit = self.vertex_hit(&project, cp);
                        self.vertex_select(hit, mods.shift || ctrl);
                    }
                    Tool::Select | Tool::Texture | Tool::Vertex => {
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
        // context menu: right-drag pans, so only a right click without a drag gets here.
        // Like Hammer, right-clicking an object that isn't selected selects it first.
        if resp.secondary_clicked() {
            if let Some(cp) = resp.interact_pointer_pos() {
                if let Some(id) = self.pick_2d(vi, wpos(cp), 3.0 / view.zoom).filter(|id| !self.sel.contains(id)) {
                    self.set_sel([id].into_iter().collect());
                }
            }
        }
        resp.context_menu(|ui| self.view_context_menu(ui));

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
        self.status = format!("Created {class}");
    }
}
