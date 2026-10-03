//! Per-type property widgets.

use super::angles::{angles_editor, is_vector_angles, yaw_editor};
use crate::formats::fgd::Prop;
use crate::formats::vmf::{self};
use eframe::egui::{self};

pub(crate) fn parse_color255(s: &str) -> ([u8; 3], i32) {
    let n: Vec<i32> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    let g = |i: usize, d: i32| n.get(i).copied().unwrap_or(d);
    ([g(0, 255).clamp(0, 255) as u8, g(1, 255).clamp(0, 255) as u8, g(2, 255).clamp(0, 255) as u8], g(3, 200))
}

/// Returns true when the value changed.
pub(crate) fn prop_editor(ui: &mut egui::Ui, prop: &Prop, value: &mut String, targets: &[String]) -> bool {
    let mut changed = false;
    // The FGD declares `angles(angle)` / `movedir(angle)` although the value is "pitch yaw roll".
    let ty = if is_vector_angles(&prop.name, &prop.ty, value, &prop.default) { "angles" } else { prop.ty.as_str() };
    match ty {
        "choices" => {
            let label = prop
                .choices
                .iter()
                .find(|c| c.value == *value)
                .map(|c| format!("{} ({})", c.label, c.value))
                .unwrap_or_else(|| if value.is_empty() { "(default)".to_string() } else { value.clone() });
            egui::ComboBox::from_id_salt(&prop.name).selected_text(label).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                for c in &prop.choices {
                    if ui.selectable_label(*value == c.value, format!("{} ({})", c.label, c.value)).clicked() {
                        *value = c.value.clone();
                        changed = true;
                    }
                }
            });
        }
        "integer" => {
            let mut v: i64 = value.parse().unwrap_or(prop.default.parse().unwrap_or(0));
            if ui.add(egui::DragValue::new(&mut v).speed(1.0)).changed() {
                *value = v.to_string();
                changed = true;
            }
        }
        "angles" => changed |= angles_editor(ui, value),
        "angle" => changed |= yaw_editor(ui, value),
        "float" => {
            let mut v: f64 = value.parse().unwrap_or(prop.default.parse().unwrap_or(0.0));
            if ui.add(egui::DragValue::new(&mut v).speed(0.1)).changed() {
                *value = vmf::fmt(v);
                changed = true;
            }
        }
        "boolean" => {
            let mut b = value == "1";
            if ui.checkbox(&mut b, "").changed() {
                *value = if b { "1" } else { "0" }.into();
                changed = true;
            }
        }
        "color255" => {
            let (mut rgb, mut br) = parse_color255(if value.is_empty() { &prop.default } else { value });
            ui.horizontal(|ui| {
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    changed = true;
                }
                if ui.add(egui::DragValue::new(&mut br).prefix("I ")).changed() {
                    changed = true;
                }
            });
            if changed {
                *value = format!("{} {} {} {}", rgb[0], rgb[1], rgb[2], br);
            }
        }
        "vector" | "origin" => {
            let mut v = vmf::parse_vec3(value).unwrap_or_default();
            let mut ch = false;
            ui.horizontal(|ui| {
                for i in 0..3 {
                    ch |= ui.add(egui::DragValue::new(&mut v[i]).speed(0.5)).changed();
                }
            });
            if ch {
                *value = vmf::fmt_vec3(v);
                changed = true;
            }
        }
        "target_destination" | "target_source" | "target_name_or_class" => {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(value).desired_width(ui.available_width() - 28.0)).changed();
                ui.menu_button("v", |ui| {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for t in targets {
                            if ui.button(t).clicked() {
                                *value = t.clone();
                                changed = true;
                                ui.close();
                            }
                        }
                    });
                });
            });
        }
        _ => {
            changed |= ui.add(egui::TextEdit::singleline(value).desired_width(ui.available_width() - 8.0)).changed();
        }
    }
    changed
}
