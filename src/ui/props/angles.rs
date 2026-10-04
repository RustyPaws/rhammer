//! Angles editor: labelled pitch/yaw/roll, a yaw dial and direction presets.

use crate::editor::direction::{angles_matrix, yaw_direction};
use crate::formats::vmf;
use eframe::egui::{self, Color32, Sense, Stroke, Vec2};
use glam::DVec3;

const DIAL_SIZE: f32 = 52.0;

fn wrap(a: f64) -> f64 {
    let w = (a + 180.0).rem_euclid(360.0) - 180.0;
    if w == -180.0 { 180.0 } else { w }
}

/// Is this value/key an angles-style vector (pitch yaw roll) rather than a single yaw?
pub fn is_vector_angles(key: &str, ty: &str, value: &str, default: &str) -> bool {
    key.eq_ignore_ascii_case("angles")
        || key.eq_ignore_ascii_case("movedir")
        || (ty == "angle" && (value.split_whitespace().count() == 3 || default.split_whitespace().count() == 3))
}

/// A painted dial: the arrow shows where `dir` points in the XY plane (0° = +X, counter-clockwise).
/// Returns the new yaw when the user clicks or drags it.
fn dial(ui: &mut egui::Ui, dir: DVec3, up_down: bool) -> Option<f64> {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(DIAL_SIZE), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    let c = rect.center();
    let r = DIAL_SIZE / 2.0 - 3.0;
    let accent = ui.visuals().selection.bg_fill;
    p.circle_filled(c, r, ui.visuals().extreme_bg_color);
    p.circle_stroke(c, r, Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color));
    for (i, l) in ["E", "N", "W", "S"].iter().enumerate() {
        let a = (i as f32) * std::f32::consts::FRAC_PI_2;
        let q = c + Vec2::new(a.cos(), -a.sin()) * (r - 7.0);
        p.text(q, egui::Align2::CENTER_CENTER, l, egui::FontId::proportional(8.0), Color32::from_gray(130));
    }
    let flat = Vec2::new(dir.x as f32, -dir.y as f32);
    if flat.length() > 0.05 {
        let tip = c + flat.normalized() * (r - 3.0);
        p.line_segment([c, tip], Stroke::new(2.0, accent));
        p.circle_filled(tip, 3.0, accent);
    } else {
        // straight up / down: show a ring (up) or dot with cross (down)
        p.circle_stroke(c, 5.0, Stroke::new(2.0, accent));
        if up_down {
            p.circle_filled(c, 2.0, accent);
        }
    }
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let d: Vec2 = pos - c;
            if d.length() > 2.0 {
                let mut yaw = (-d.y as f64).atan2(d.x as f64).to_degrees();
                if ui.input(|i| i.modifiers.shift) {
                    yaw = (yaw / 15.0).round() * 15.0;
                }
                return Some(wrap(yaw));
            }
        }
    }
    None
}

/// Editor for a "pitch yaw roll" value. Returns true when `value` changed.
pub fn angles_editor(ui: &mut egui::Ui, value: &mut String) -> bool {
    let mut a = vmf::parse_vec3(value).unwrap_or_default();
    let mut changed = false;
    ui.vertical(|ui| {
        // the dial goes beside the pitch/yaw/roll fields, or above them when there is no room:
        // anything wider than the panel would widen every field after it
        let beside = ui.available_width() >= DIAL_SIZE + 130.0;
        let layout = if beside { egui::Layout::left_to_right(egui::Align::Min) } else { egui::Layout::top_down(egui::Align::Min) };
        ui.with_layout(layout, |ui| {
            let dir = angles_matrix(a) * DVec3::X;
            let vertical = dir.x.abs() < 0.05 && dir.y.abs() < 0.05;
            if let Some(yaw) = dial(ui, dir, dir.z < 0.0 && vertical) {
                a.y = yaw;
                changed = true;
            }
            ui.vertical(|ui| {
                for (i, name) in ["Pitch", "Yaw", "Roll"].iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_sized([34.0, 16.0], egui::Label::new(egui::RichText::new(*name).small()));
                        let r = ui.add(egui::DragValue::new(&mut a[i]).speed(1.0).suffix("°"));
                        if r.changed() {
                            a[i] = wrap(a[i]);
                            changed = true;
                        }
                    });
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            let presets: [(&str, DVec3); 7] = [
                ("Up", DVec3::new(-90.0, 0.0, 0.0)),
                ("Down", DVec3::new(90.0, 0.0, 0.0)),
                ("E", DVec3::new(0.0, 0.0, 0.0)),
                ("N", DVec3::new(0.0, 90.0, 0.0)),
                ("W", DVec3::new(0.0, 180.0, 0.0)),
                ("S", DVec3::new(0.0, -90.0, 0.0)),
                ("Reset", DVec3::ZERO),
            ];
            for (label, v) in presets {
                if ui.small_button(label).clicked() {
                    // horizontal presets keep the current pitch/roll untouched only for reset
                    a = if label.len() == 1 { DVec3::new(0.0, v.y, a.z) } else { v };
                    changed = true;
                }
            }
        });
    });
    if changed {
        *value = vmf::fmt_vec3(a);
    }
    changed
}

/// Editor for a single yaw (`angle` key): dial, drag value and Up/Down (-1/-2).
pub fn yaw_editor(ui: &mut egui::Ui, value: &mut String) -> bool {
    let mut yaw: f64 = value.trim().parse().unwrap_or(0.0);
    let mut changed = false;
    ui.horizontal(|ui| {
        let dir = yaw_direction(yaw);
        if let Some(y) = dial(ui, dir, yaw == -2.0) {
            yaw = y;
            changed = true;
        }
        ui.vertical(|ui| {
            let label = match yaw {
                y if y == -1.0 => "Up".to_string(),
                y if y == -2.0 => "Down".to_string(),
                _ => String::new(),
            };
            let r = ui.add(egui::DragValue::new(&mut yaw).speed(1.0).custom_formatter(move |v, _| if label.is_empty() { format!("{v}°") } else { label.clone() }));
            if r.changed() {
                changed = true;
            }
            ui.horizontal(|ui| {
                if ui.small_button("Up").clicked() {
                    yaw = -1.0;
                    changed = true;
                }
                if ui.small_button("Down").clicked() {
                    yaw = -2.0;
                    changed = true;
                }
            });
        });
    });
    if changed {
        *value = vmf::fmt(if yaw < 0.0 && yaw != -1.0 && yaw != -2.0 { wrap(yaw) } else { yaw });
    }
    changed
}
