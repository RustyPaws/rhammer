//! Small 2D overlay shapes: direction arrows and the origin marker.

use eframe::egui::{Color32, Painter, Pos2, Stroke, Vec2};

/// Arrow from `from` along the screen-space direction `dir` (y down). A direction that is
/// (nearly) perpendicular to the view plane is drawn as a ringed dot instead.
pub(crate) fn draw_direction(painter: &Painter, from: Pos2, dir: Vec2, len: f32, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    if dir.length() < 0.05 {
        painter.circle_stroke(from, 5.0, stroke);
        painter.circle_filled(from, 1.5, color);
        return;
    }
    let d = dir.normalized();
    let tip = from + d * len;
    painter.line_segment([from, tip], stroke);
    let side = Vec2::new(-d.y, d.x);
    let back = tip - d * 7.0;
    painter.line_segment([tip, back + side * 4.0], stroke);
    painter.line_segment([tip, back - side * 4.0], stroke);
}

/// Crosshair marking an entity origin.
pub(crate) fn draw_origin_marker(painter: &Painter, c: Pos2, color: Color32) {
    let s = Stroke::new(1.5, color);
    painter.circle_stroke(c, 5.0, s);
    painter.line_segment([c - Vec2::new(8.0, 0.0), c + Vec2::new(8.0, 0.0)], s);
    painter.line_segment([c - Vec2::new(0.0, 8.0), c + Vec2::new(0.0, 8.0)], s);
}
