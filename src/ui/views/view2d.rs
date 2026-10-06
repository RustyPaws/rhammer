//! 2D orthographic viewports: drawing, picking and mouse interaction.

use crate::app::*;
use crate::editor::doc::{Obj, Sel, Xform};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use glam::{DQuat, DVec3};
use super::*;
use crate::editor::direction::entity_direction;
use crate::config::View2dStyle;

/// One orthographic view for the current frame: which view it is, its screen projection and
/// the world axes it shows (`ua` right, `va` up, `wa` into the screen).
struct View2d {
    vi: usize,
    rect: Rect,
    pr: Proj,
    ua: usize,
    va: usize,
    wa: usize,
    style: View2dStyle,
}

impl View2d {
    fn new(app: &App, rect: Rect, vi: usize) -> View2d {
        let (ua, va, wa) = axes(vi);
        let v = app.views[vi];
        View2d { vi, rect, pr: Proj { rect, center: v.center, zoom: v.zoom }, ua, va, wa, style: app.settings.editor.view2d.clone() }
    }

    fn sel_color(&self) -> Color32 {
        let [r, g, b] = self.style.sel_color;
        Color32::from_rgb(r, g, b)
    }

    /// A grid line color scaled by the user's grid brightness.
    fn grid_color(&self, base: [u8; 3]) -> Color32 {
        let k = self.style.grid_brightness;
        let f = |v: u8| (v as f32 * k).round().clamp(0.0, 255.0) as u8;
        Color32::from_rgb(f(base[0]), f(base[1]), f(base[2]))
    }

    fn screen(&self, p: DVec3) -> Pos2 {
        self.pr.to_screen(p[self.ua], p[self.va])
    }

    /// The world point at view coordinates (u, v), 0 on the depth axis.
    fn world(&self, u: f64, v: f64) -> DVec3 {
        let mut p = DVec3::ZERO;
        p[self.ua] = u;
        p[self.va] = v;
        p
    }

    fn screen_box(&self, b: (DVec3, DVec3)) -> Rect {
        Rect::from_two_pos(self.screen(b.0), self.screen(b.1))
    }

    /// Pick tolerance in world units (3 pixels).
    fn pick_tol(&self) -> f64 {
        3.0 / self.pr.zoom
    }

    /// The box overlaps the visible part of the view.
    fn in_view(&self, a: DVec3, b: DVec3) -> bool {
        let (u0, v1) = self.pr.to_world(self.rect.left_top());
        let (u1, v0) = self.pr.to_world(self.rect.right_bottom());
        b[self.ua] >= u0 && a[self.ua] <= u1 && b[self.va] >= v0 && a[self.va] <= v1
    }

    /// The view point `w` lies inside the box (corners in any order).
    fn box_contains(&self, b: (DVec3, DVec3), w: (f64, f64)) -> bool {
        let (ua, va) = (self.ua, self.va);
        w.0 >= b.0[ua].min(b.1[ua]) && w.0 <= b.0[ua].max(b.1[ua]) && w.1 >= b.0[va].min(b.1[va]) && w.1 <= b.0[va].max(b.1[va])
    }

    /// The resize handle of box `b` under the screen point, as (hx, hy): -1 = min side in
    /// world u / v, +1 = max side, 0 = middle.
    fn handle_at(&self, pos: Pos2, b: (DVec3, DVec3)) -> Option<(i32, i32)> {
        let r = self.screen_box(b);
        for (x, hx) in [(r.left(), -1), (r.center().x, 0), (r.right(), 1)] {
            for (y, hy) in [(r.bottom(), -1), (r.center().y, 0), (r.top(), 1)] {
                if (hx, hy) != (0, 0) && (pos.x - x).abs() <= 6.0 && (pos.y - y).abs() <= 6.0 {
                    return Some((hx, hy));
                }
            }
        }
        None
    }
}

/// Bounds of the box `a..b` after the preview transform, if any.
fn xform_box(xf: Option<&Xform>, a: DVec3, b: DVec3) -> (DVec3, DVec3) {
    match xf {
        Some(x) => {
            let (p, q) = (x.point(a), x.point(b));
            (p.min(q), p.max(q))
        }
        None => (a, b),
    }
}

/// Box outline with resize handles (or round rotation handles) and Hammer-style dimensions.
fn draw_handles(painter: &egui::Painter, c: &View2d, b: (DVec3, DVec3), color: Color32, rot: bool) {
    let r = c.screen_box(b);
    painter.rect_stroke(r, 0.0, Stroke::new(1.0, color.linear_multiply(0.6)), egui::StrokeKind::Outside);
    for (x, hx) in [(r.left(), -1), (r.center().x, 0), (r.right(), 1)] {
        for (y, hy) in [(r.bottom(), -1), (r.center().y, 0), (r.top(), 1)] {
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
    // width above the box, height left of it
    let s = b.1 - b.0;
    let fmt = |v: f64| {
        let v = v.abs();
        if (v - v.round()).abs() < 0.005 { format!("{:.0}", v) } else { format!("{:.2}", v) }
    };
    let font = egui::FontId::monospace(11.0);
    painter.text(r.center_top() - Vec2::new(0.0, 8.0), egui::Align2::CENTER_BOTTOM, fmt(s[c.ua]), font.clone(), color);
    painter.text(r.left_center() - Vec2::new(8.0, 0.0), egui::Align2::RIGHT_CENTER, fmt(s[c.va]), font, color);
}

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

    /// One orthographic view: navigation, drawing, then mouse interaction for the active tool.
    pub(crate) fn view2d_ui(&mut self, ui: &mut egui::Ui, rect: Rect, vi: usize) {
        let id = ui.id().with(("view2d", vi));
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        self.view_busy |= resp.dragged();
        let hover = resp.hover_pos().filter(|p| rect.contains(*p));
        let mods = ui.input(|i| i.modifiers);
        self.view2d_navigate(ui, &resp, rect, vi, hover);
        let c = View2d::new(self, rect, vi);

        // ---- drawing ----
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(c.style.background[0], c.style.background[1], c.style.background[2]));
        if self.show_grid {
            self.draw_grid_2d(&painter, &c);
        }
        let xf = self.drag_xform();
        self.draw_objects_2d(&painter, &c, xf.as_ref());
        let sel_b = self.doc.sel_bounds(&self.sel, &self.fgd);
        let origin = self.origin_marker();
        self.draw_overlays_2d(&painter, &c, sel_b, origin, xf.as_ref());

        // cursor feedback over the resize handles
        if let (Some(h), Some(b), Tool::Select) = (hover, sel_b, self.tool) {
            if let Some((hx, hy)) = c.handle_at(h, b) {
                ui.ctx().set_cursor_icon(match (hx, hy) {
                    (0, _) => egui::CursorIcon::ResizeVertical,
                    (_, 0) => egui::CursorIcon::ResizeHorizontal,
                    (a, b) if a * b > 0 => egui::CursorIcon::ResizeNeSw,
                    _ => egui::CursorIcon::ResizeNwSe,
                });
            }
        }

        // ---- interaction ----
        if resp.drag_started_by(egui::PointerButton::Primary) {
            if let Some(po) = ui.input(|i| i.pointer.press_origin()).filter(|p| rect.contains(*p)) {
                self.drag_start_2d(&c, po, mods, sel_b, origin);
            }
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            if let Some(cp) = resp.interact_pointer_pos() {
                self.drag_update_2d(&c, cp);
            }
        }
        if resp.drag_stopped_by(egui::PointerButton::Primary) {
            self.drag_end_2d(&c, mods);
        }
        if resp.clicked_by(egui::PointerButton::Primary) {
            if let Some(cp) = resp.interact_pointer_pos() {
                self.click_2d(&c, cp, mods);
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
                self.apply_xform(Xform::Translate(c.world(d.0 * self.grid, d.1 * self.grid)));
            }
        }
        // context menu: right-drag pans, so only a right click without a drag gets here.
        // Like Hammer, right-clicking an object that isn't selected selects it first.
        if resp.secondary_clicked() {
            if let Some(cp) = resp.interact_pointer_pos() {
                if let Some(id) = self.pick_2d(vi, c.pr.to_world(cp), c.pick_tol()).filter(|id| !self.sel.contains(id)) {
                    self.set_sel([id].into_iter().collect());
                }
            }
        }
        resp.context_menu(|ui| self.view_context_menu(ui));

        self.view_hotkeys(ui, hover.is_some(), vi + 1);
    }

    /// Wheel zoom around the cursor, middle / right drag pans. Also reports the hovered world point.
    fn view2d_navigate(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, vi: usize, hover: Option<Pos2>) {
        let view = self.views[vi];
        let pr = Proj { rect, center: view.center, zoom: view.zoom };
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
            let (ua, va, _) = axes(vi);
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
    }

    fn draw_grid_2d(&self, painter: &egui::Painter, c: &View2d) {
        let (pr, rect) = (&c.pr, c.rect);
        let (u0, v1) = pr.to_world(rect.left_top());
        let (u1, v0) = pr.to_world(rect.right_bottom());
        let mut step = self.grid;
        while step * pr.zoom < 6.0 {
            step *= 2.0;
        }
        let draw_lines = |step: f64, color: Color32| {
            let stroke = Stroke::new(1.0, color);
            let mut x = (u0 / step).floor() * step;
            while x <= u1 {
                let sx = pr.to_screen(x, 0.0).x;
                painter.line_segment([Pos2::new(sx, rect.top()), Pos2::new(sx, rect.bottom())], stroke);
                x += step;
            }
            let mut y = (v0 / step).floor() * step;
            while y <= v1 {
                let sy = pr.to_screen(0.0, y).y;
                painter.line_segment([Pos2::new(rect.left(), sy), Pos2::new(rect.right(), sy)], stroke);
                y += step;
            }
        };
        draw_lines(step, c.grid_color([24, 24, 30]));
        let mut big = 64.0;
        while big < step {
            big *= 2.0;
        }
        if big * pr.zoom >= 8.0 {
            draw_lines(big, c.grid_color([40, 40, 54]));
        }
        draw_lines(1024.0, c.grid_color([70, 70, 40]));
        // axes
        let o = pr.to_screen(0.0, 0.0);
        painter.line_segment([Pos2::new(o.x, rect.top()), Pos2::new(o.x, rect.bottom())], Stroke::new(1.0, Color32::from_rgb(40, 90, 40)));
        painter.line_segment([Pos2::new(rect.left(), o.y), Pos2::new(rect.right(), o.y)], Stroke::new(1.0, Color32::from_rgb(90, 40, 40)));
    }

    /// Brushes, point entities (boxes / sprites / instances), names and facing arrows.
    /// `xf` is the transform being dragged, previewed on the selected objects.
    fn draw_objects_2d(&self, painter: &egui::Painter, c: &View2d, xf: Option<&Xform>) {
        let (ua, va) = (c.ua, c.va);
        let mut shapes: Vec<egui::Shape> = Vec::new();
        let sel_dim = Color32::from_rgb(190, 190, 200);
        let sel = c.sel_color();
        let draw_solid = |shapes: &mut Vec<egui::Shape>, polys: &[Vec<DVec3>], color: Color32, width: f32, xf: Option<&Xform>| {
            for poly in polys {
                if poly.len() < 2 {
                    continue;
                }
                let pts: Vec<Pos2> = poly.iter().map(|v| c.screen(xf.map(|x| x.point(*v)).unwrap_or(*v))).collect();
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
                let stroke = Stroke::new(width, color);
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
            let preview = if selected { xf } else { None };
            let color = if selected { sel } else { color };
            let width = if selected { c.style.sel_width } else { 1.0 };
            if is_point {
                let e = ent.unwrap();
                let (a, b) = self.doc.ent_bounds(e, &self.fgd);
                let (a, b) = xform_box(preview, a, b);
                if !c.in_view(a, b) {
                    continue;
                }
                let r = c.screen_box((a, b));
                let r = if r.width() < 4.0 { Rect::from_center_size(r.center(), Vec2::splat(4.0)) } else { r };
                if let Some(ig) = self.inst.get(&id) {
                    // instance: draw its contents, only outline the bounds
                    let line = if selected { sel } else { Color32::from_rgb(120, 130, 160) };
                    if std::env::var("RHAMMER_DEBUG").is_ok() && c.vi == 0 {
                        for (fi, poly) in ig.polys.iter().enumerate() {
                            let le = (0..poly.len()).map(|i| (poly[i] - poly[(i + 1) % poly.len()]).length()).fold(0.0, f64::max);
                            if le > 2500.0 {
                                eprintln!("INST ent {} file {:?} face {} edge {:.0} mat {} poly {:?}", id, e.get("file"), fi, le, ig.faces[fi].material, poly);
                            }
                        }
                    }
                    draw_solid(&mut shapes, &ig.polys, line, width, preview);
                    painter.rect_stroke(r, 0.0, Stroke::new(width, color.linear_multiply(0.6)), egui::StrokeKind::Inside);
                } else if let Some(Some(tex)) = crate::editor::doc::entity_sprite(e, &self.fgd).and_then(|s| self.sprite_tex.get(&s)) {
                    // icon sprite: at least 16px, keep the box outline for selection / hit feedback
                    let side = r.width().max(r.height()).max(16.0);
                    let ir = Rect::from_center_size(r.center(), Vec2::splat(side));
                    painter.image(tex.id(), ir, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    if selected {
                        painter.rect_stroke(ir, 0.0, Stroke::new(width, color), egui::StrokeKind::Inside);
                    }
                } else {
                    if c.style.fill_point_entities {
                        painter.rect_filled(r, 0.0, color.linear_multiply(0.35));
                    }
                    painter.rect_stroke(r, 0.0, Stroke::new(width, color), egui::StrokeKind::Inside);
                }
                // facing direction
                if selected && !self.inst.contains_key(&id) {
                    if let Some(f) = entity_direction(e, &self.fgd) {
                        let len = (r.width().max(r.height()) * 0.8).clamp(14.0, 60.0);
                        overlays::draw_direction(painter, r.center(), Vec2::new(f[ua] as f32, -f[va] as f32), len, color);
                    }
                }
                if self.show_entity_names && c.pr.zoom > 0.35 {
                    let label = e.get("targetname").filter(|t| !t.is_empty()).unwrap_or(e.classname());
                    painter.text(r.right_top() + Vec2::new(3.0, 0.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(10.0), color);
                }
            } else {
                for sid in solids_ids {
                    let Some(g) = self.doc.geo.get(&sid) else { continue };
                    if !g.valid {
                        continue;
                    }
                    let (a, b) = xform_box(preview, g.min, g.max);
                    if !c.in_view(a, b) {
                        continue;
                    }
                    draw_solid(&mut shapes, &g.polys, color, width, preview);
                }
                // brush entity direction (door move direction, push direction …)
                if let Some((e, f)) = ent.filter(|_| selected).and_then(|e| entity_direction(e, &self.fgd).map(|f| (e, f))) {
                    let (a, b) = self.doc.ent_bounds(e, &self.fgd);
                    let (a, b) = xform_box(preview, a, b);
                    if c.in_view(a, b) {
                        let size = b - a;
                        let extent = (f.x * size.x).abs() + (f.y * size.y).abs() + (f.z * size.z).abs();
                        let len = ((extent * 0.5 * c.pr.zoom) as f32).clamp(18.0, 160.0);
                        overlays::draw_direction(painter, c.screen((a + b) * 0.5), Vec2::new(f[ua] as f32, -f[va] as f32), len, color);
                    }
                }
            }
        }
        painter.extend(shapes);
    }

    /// The origin marker of the single selected brush entity (drag it to set `origin`).
    fn origin_marker(&self) -> Option<(u32, DVec3)> {
        (self.tool == Tool::Select && self.sel.len() == 1)
            .then(|| self.sel.iter().next().copied())
            .flatten()
            .and_then(|id| self.doc.entity(id).filter(|e| !e.solids.is_empty()).map(|e| (id, e)))
            .map(|(id, e)| {
                let (a, b) = self.doc.ent_bounds(e, &self.fgd);
                (id, if e.get("origin").is_some() { e.origin() } else { (a + b) * 0.5 })
            })
    }

    /// Tool feedback on top of the objects: selection handles, origin marker, block preview,
    /// vertices, the box selection and the clip line.
    fn draw_overlays_2d(&self, painter: &egui::Painter, c: &View2d, sel_b: Option<(DVec3, DVec3)>, origin: Option<(u32, DVec3)>, xf: Option<&Xform>) {
        let vi = c.vi;
        if let Some((id, mut o)) = origin {
            if let Some(Drag::Origin { view: v, id: did, cur, .. }) = &self.drag {
                if *v == vi && *did == id {
                    o[c.ua] = cur.0;
                    o[c.va] = cur.1;
                }
            }
            overlays::draw_origin_marker(painter, c.screen(o), Color32::from_rgb(80, 220, 255));
        }
        if let Some(b) = sel_b {
            let b = match xf {
                Some(Xform::Rotate { .. }) | None => b,
                Some(_) => xform_box(xf, b.0, b.1),
            };
            if self.tool == Tool::Select {
                draw_handles(painter, c, b, c.sel_color(), self.rot_mode);
            } else {
                // other tools: a thin box so the selection is still easy to find
                painter.rect_stroke(c.screen_box(b), 0.0, Stroke::new(1.0, c.sel_color().linear_multiply(0.6)), egui::StrokeKind::Outside);
            }
        }
        if let (Tool::Block, Some(b)) = (self.tool, self.block) {
            draw_handles(painter, c, b, Color32::YELLOW, false);
        }
        if self.tool == Tool::Vertex {
            let delta = match &self.drag {
                Some(Drag::Vertex { view: v, delta, .. }) if *v == vi => Some(*delta),
                _ => None,
            };
            self.vertex_draw(painter, &|p| Some(c.screen(p)), delta);
        }
        if let Some(Drag::BoxSel { view: v, start, cur }) = &self.drag {
            if *v == vi {
                let r = Rect::from_two_pos(c.pr.to_screen(start.0, start.1), c.pr.to_screen(cur.0, cur.1));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(120, 160, 255)), egui::StrokeKind::Inside);
            }
        }
        if self.tool == Tool::Clip && self.clip.view == vi {
            if let (Some(p0), Some(p1)) = (self.clip.p0, self.clip.p1) {
                for (polys, keep) in self.clip_preview() {
                    let color = if keep { Color32::WHITE } else { Color32::from_rgb(255, 40, 40) };
                    for poly in &polys {
                        for i in 0..poly.len() {
                            painter.line_segment([c.screen(poly[i]), c.screen(poly[(i + 1) % poly.len()])], Stroke::new(1.5, color));
                        }
                    }
                }
                let a = c.pr.to_screen(p0.0, p0.1);
                let b = c.pr.to_screen(p1.0, p1.1);
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
    }

    fn snap2(&self, p: (f64, f64)) -> (f64, f64) {
        (self.snapv(p.0), self.snapv(p.1))
    }

    /// Left drag begins at `po`: decide what the drag does for the active tool.
    fn drag_start_2d(&mut self, c: &View2d, po: Pos2, mods: egui::Modifiers, sel_b: Option<(DVec3, DVec3)>, origin: Option<(u32, DVec3)>) {
        let (vi, ua, va, wa) = (c.vi, c.ua, c.va, c.wa);
        let w = c.pr.to_world(po);
        let ctrl = mods.command;
        let move_drag = |s: &App| Drag::Move { view: vi, start: s.snap2(w), delta: (0.0, 0.0), clone: mods.shift };
        match self.tool {
            Tool::Select | Tool::Texture => {
                let hit_origin = origin.filter(|(_, o)| self.tool == Tool::Select && (c.screen(*o) - po).length() <= 8.0);
                if let Some((id, o)) = hit_origin {
                    self.drag = Some(Drag::Origin { view: vi, id, orig: o, cur: (o[ua], o[va]) });
                } else if let (Some(b), true) = (sel_b, self.tool == Tool::Select) {
                    if let Some((hx, hy)) = c.handle_at(po, b).filter(|(hx, hy)| !self.rot_mode || (*hx != 0 && *hy != 0)) {
                        if self.rot_mode {
                            let center = (b.0 + b.1) * 0.5;
                            let start_angle = (w.1 - center[va]).atan2(w.0 - center[ua]);
                            self.drag = Some(Drag::Rotate { view: vi, center, start_angle, angle: 0.0 });
                        } else {
                            self.drag = Some(Drag::Resize { view: vi, hx, hy, orig: b, cur: self.snap2(w) });
                        }
                    } else if c.box_contains(b, w) && !ctrl {
                        self.drag = Some(move_drag(self));
                    } else if let Some(id) = self.pick_2d(vi, w, c.pick_tol()).filter(|_| !ctrl) {
                        if !self.sel.contains(&id) {
                            self.set_sel([id].into_iter().collect());
                        }
                        self.drag = Some(move_drag(self));
                    } else {
                        self.drag = Some(Drag::BoxSel { view: vi, start: w, cur: w });
                    }
                } else if self.tool == Tool::Select {
                    if let Some(id) = self.pick_2d(vi, w, c.pick_tol()).filter(|_| !ctrl) {
                        self.set_sel([id].into_iter().collect());
                        self.drag = Some(move_drag(self));
                    } else {
                        self.drag = Some(Drag::BoxSel { view: vi, start: w, cur: w });
                    }
                }
            }
            Tool::Block => {
                let sw = self.snap2(w);
                if let Some(b) = self.block {
                    if let Some((hx, hy)) = c.handle_at(po, b) {
                        self.drag = Some(Drag::BlockResize { view: vi, hx, hy, orig: b });
                        return;
                    }
                    if c.box_contains(b, w) {
                        self.drag = Some(Drag::BlockMove { view: vi, start: sw, orig: b });
                        return;
                    }
                }
                // start a new block on the grid, keeping the depth of the previous one
                let (mut a, mut b) = self.block.unwrap_or_else(|| {
                    let (mut a, mut b) = (DVec3::ZERO, DVec3::ZERO);
                    a[wa] = 0.0;
                    b[wa] = 64.0f64.max(self.grid);
                    (a, b)
                });
                a[ua] = sw.0;
                a[va] = sw.1;
                b[ua] = sw.0;
                b[va] = sw.1;
                self.block = Some((a, b));
                self.drag = Some(Drag::BlockNew { view: vi, start: sw });
            }
            Tool::Clip => {
                let sw = self.snap2(w);
                self.clip.view = vi;
                self.clip.p0 = Some(sw);
                self.clip.p1 = Some(sw);
                self.drag = Some(Drag::ClipLine { view: vi });
            }
            Tool::Vertex => {
                let hit = self.vertex_hit(&|p| Some(c.screen(p)), po);
                if hit.is_empty() {
                    self.drag = Some(Drag::BoxSel { view: vi, start: w, cur: w });
                } else {
                    self.vertex_select(hit.clone(), mods.shift || ctrl);
                    if let Some((_, grab)) = hit.into_iter().find(|(id, p)| self.vtx.sel.contains(&(*id, *p))) {
                        self.drag = Some(Drag::Vertex { view: vi, plane: [ua, va], start: c.world(w.0, w.1), grab, delta: DVec3::ZERO });
                    }
                }
            }
            Tool::Entity => {}
        }
    }

    /// The left drag moved to `cp`: update the drag in progress.
    fn drag_update_2d(&mut self, c: &View2d, cp: Pos2) {
        let (vi, ua, va) = (c.vi, c.ua, c.va);
        let w = c.pr.to_world(cp);
        let sw = self.snap2(w);
        let snap = self.snap;
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
        let mut block_upd: Option<(DVec3, DVec3)> = None;
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
                    -1 => a[ua] = sw.0,
                    1 => b[ua] = sw.0,
                    _ => {}
                }
                match hy.signum() {
                    -1 => a[va] = sw.1,
                    1 => b[va] = sw.1,
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

    /// The left drag ended: commit what it did.
    fn drag_end_2d(&mut self, c: &View2d, mods: egui::Modifiers) {
        let additive = mods.shift || mods.command;
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
                let zoom = self.views[view].zoom;
                if self.tool == Tool::Vertex && !self.sel.is_empty() {
                    let area = Rect::from_two_pos(c.pr.to_screen(u0, v0), c.pr.to_screen(u1, v1));
                    self.vertex_box_select(&|p| Some(c.screen(p)), area, additive);
                } else if (u1 - u0) * zoom > 3.0 || (v1 - v0) * zoom > 3.0 {
                    let mut found = Sel::new();
                    for id in self.doc.all_ids() {
                        if self.doc.is_hidden(id) {
                            continue;
                        }
                        let boxes = self.obj_boxes(id);
                        if !boxes.is_empty() && boxes.iter().all(|(a, b)| a[ua] >= u0 && b[ua] <= u1 && a[va] >= v0 && b[va] <= v1) {
                            found.insert(id);
                        }
                    }
                    if additive {
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

    /// A left click without a drag: select, toggle rotation handles or place an entity.
    fn click_2d(&mut self, c: &View2d, cp: Pos2, mods: egui::Modifiers) {
        let additive = mods.shift || mods.command;
        let w = c.pr.to_world(cp);
        let project = |p: DVec3| Some(c.screen(p));
        match self.tool {
            Tool::Vertex if !self.vertex_hit(&project, cp).is_empty() => {
                let hit = self.vertex_hit(&project, cp);
                self.vertex_select(hit, additive);
            }
            Tool::Select | Tool::Texture | Tool::Vertex => match self.pick_2d(c.vi, w, c.pick_tol()) {
                Some(id) => {
                    if additive {
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
                    if !additive {
                        self.set_sel(Sel::new());
                    }
                    self.rot_mode = false;
                }
            },
            Tool::Entity => {
                let p = c.world(self.snapv(w.0), self.snapv(w.1));
                self.place_entity(p);
            }
            _ => {}
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
