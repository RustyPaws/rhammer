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

/// A combo-style button whose popup has a filter field above a scrolled list of `items`. Returns
/// the item clicked this frame. The caller keeps `filter`, so it survives closing the popup.
///
/// egui lays a popup out inside the size it had last frame, so an auto-sized list can shrink but
/// never grow back. The list height is therefore set explicitly from the match count.
pub(crate) fn filter_combo<'a>(ui: &mut egui::Ui, id_salt: &str, selected: &str, width: Option<f32>, filter: &mut String, items: impl IntoIterator<Item = &'a str>) -> Option<String> {
    const MAX_HEIGHT: f32 = 360.0;
    let mut picked = None;
    let all: Vec<&str> = items.into_iter().collect();
    let btn_id = ui.make_persistent_id(id_salt);
    let (open_id, focus_id) = (btn_id.with("open"), btn_id.with("filter"));
    let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);

    // Trailing spaces leave room for the arrow painted below (the default fonts lack a ▾ glyph).
    let mut button = egui::Button::new(format!("{selected}     ")).wrap_mode(egui::TextWrapMode::Truncate);
    if let Some(w) = width {
        button = button.min_size(egui::vec2(w, 0.0));
    }
    let resp = ui.add(button);
    if resp.clicked() {
        open = !open;
    }
    let c = egui::pos2(resp.rect.right() - 10.0, resp.rect.center().y);
    let color = ui.style().interact(&resp).fg_stroke.color;
    ui.painter().add(egui::Shape::convex_polygon(vec![c + egui::vec2(-4.0, -2.0), c + egui::vec2(4.0, -2.0), c + egui::vec2(0.0, 3.0)], color, egui::Stroke::NONE));

    if open {
        egui::Popup::from_response(&resp)
            .id(btn_id.with("popup"))
            .open_bool(&mut open)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(resp.rect.width())
            .show(|ui| {
                let edit = ui.add(egui::TextEdit::singleline(filter).id(focus_id).hint_text("filter…"));
                if edit.changed() {
                    ui.ctx().request_repaint();
                }
                // Focus once per opening, not during egui's invisible first-frame sizing pass.
                if ui.is_visible() && !ui.data(|d| d.get_temp::<bool>(open_id.with("focused")).unwrap_or(false)) {
                    edit.request_focus();
                    ui.data_mut(|d| d.insert_temp(open_id.with("focused"), true));
                }
                let f = filter.to_ascii_lowercase();
                let matched: Vec<&str> = all.iter().copied().filter(|n| name_matches(&f, n)).collect();
                if edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Some(n) = matched.first() {
                        picked = Some(n.to_string());
                        ui.close();
                    }
                }
                let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                let h = (matched.len().max(1) as f32 * row).min(MAX_HEIGHT);
                egui::ScrollArea::vertical().min_scrolled_height(h).max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                    for n in &matched {
                        if ui.selectable_label(*n == selected, *n).clicked() {
                            picked = Some(n.to_string());
                            ui.close();
                        }
                    }
                });
            });
    }
    if picked.is_some() {
        open = false;
    }
    if !open {
        ui.data_mut(|d| d.remove_temp::<bool>(open_id.with("focused")));
    }
    ui.data_mut(|d| d.insert_temp(open_id, open));
    picked
}

fn name_matches(filter_lower: &str, name: &str) -> bool {
    filter_lower.is_empty() || name.to_ascii_lowercase().contains(filter_lower)
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
