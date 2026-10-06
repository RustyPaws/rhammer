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
        // a name being given (target_source) has nothing useful to suggest: only references autocomplete
        "target_source" => {
            changed |= ui.add(egui::TextEdit::singleline(value).desired_width(ui.available_width() - 8.0)).changed();
        }
        "target_destination" | "target_name_or_class" => {
            ui.horizontal(|ui| {
                changed |= autocomplete_edit(ui, &format!("target_{}", prop.name), value, "", targets, ui.available_width() - 28.0);
                if let Some(t) = pick_list_button(ui, &prop.name, targets) {
                    *value = t;
                    changed = true;
                }
            });
        }
        _ => {
            changed |= ui.add(egui::TextEdit::singleline(value).desired_width(ui.available_width() - 8.0)).changed();
        }
    }
    changed
}

/// A text field that suggests matching `items` (substring, case-insensitive) while it has focus.
/// Up / Down move the highlight, Enter or a click picks, Esc closes. Returns true when the value
/// changed.
pub(crate) fn autocomplete_edit(ui: &mut egui::Ui, id_salt: &str, value: &mut String, hint: &str, items: &[String], width: f32) -> bool {
    const ROWS: usize = 40;
    let id = ui.make_persistent_id(id_salt);
    let (open_id, sel_id, dismiss_id) = (id.with("open"), id.with("sel"), id.with("dismissed"));
    let was_open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
    let mut sel = ui.data(|d| d.get_temp::<usize>(sel_id)).unwrap_or(0);
    let mut dismissed = ui.data(|d| d.get_temp::<bool>(dismiss_id)).unwrap_or(false);
    let suggestions = |text: &str| -> Vec<&String> {
        let f = text.to_ascii_lowercase();
        let m: Vec<&String> = items.iter().filter(|n| name_matches(&f, n)).take(ROWS).collect();
        // the field already holds exactly the only suggestion: nothing left to suggest
        if m.len() == 1 && m[0].eq_ignore_ascii_case(text) { vec![] } else { m }
    };
    let mut picked: Option<String> = None;
    let mut moved = false;
    if was_open && !dismissed {
        let m = suggestions(value);
        if !m.is_empty() {
            sel = sel.min(m.len() - 1);
            let (down, up, enter, esc) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                )
            });
            if down {
                sel = (sel + 1) % m.len();
                moved = true;
            }
            if up {
                sel = (sel + m.len() - 1) % m.len();
                moved = true;
            }
            if enter {
                picked = Some(m[sel].clone());
            }
            if esc {
                dismissed = true;
            }
        }
    }
    let resp = ui.add(egui::TextEdit::singleline(value).id(id).hint_text(hint).desired_width(width));
    let mut changed = resp.changed();
    if changed {
        dismissed = false;
        sel = 0;
    }
    let m: Vec<String> = suggestions(value).into_iter().cloned().collect();
    let show = resp.has_focus() && !dismissed && !m.is_empty() && picked.is_none();
    if show {
        sel = sel.min(m.len() - 1);
        // no fade-in and a width that doesn't follow the field's every pixel: both made the list blink
        let list_w = (resp.rect.width() / 8.0).round() * 8.0;
        egui::Area::new(id.with("area")).order(egui::Order::Foreground).fixed_pos(resp.rect.left_bottom()).fade_in(false).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(list_w.max(120.0));
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for (i, n) in m.iter().enumerate() {
                        let r = ui.selectable_label(i == sel, n);
                        if i == sel && moved {
                            r.scroll_to_me(None);
                        }
                        // picked on press: the click would otherwise take the focus (and the list) away first
                        if r.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
                            picked = Some(n.clone());
                        }
                    }
                });
            });
        });
    }
    if let Some(p) = picked {
        *value = p;
        changed = true;
        dismissed = true;
    }
    ui.data_mut(|d| {
        d.insert_temp(open_id, show);
        d.insert_temp(sel_id, sel);
        d.insert_temp(dismiss_id, dismissed);
    });
    changed
}

/// A small "v" button opening a filterable list of `items`. Returns the one clicked.
pub(crate) fn pick_list_button(ui: &mut egui::Ui, id_salt: &str, items: &[String]) -> Option<String> {
    let mut picked = None;
    ui.menu_button("v", |ui| {
        let fid = ui.id().with(("filter", id_salt));
        let mut f: String = ui.data_mut(|d| d.get_temp(fid).unwrap_or_default());
        let edit = ui.add(egui::TextEdit::singleline(&mut f).hint_text("filter…"));
        if ui.memory(|m| m.focused().is_none()) {
            edit.request_focus();
        }
        let fl = f.to_ascii_lowercase();
        let matched: Vec<&String> = items.iter().filter(|n| name_matches(&fl, n)).collect();
        if edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(n) = matched.first() {
                picked = Some((*n).clone());
                ui.close();
            }
        }
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for n in matched {
                if ui.button(n).clicked() {
                    picked = Some(n.clone());
                    ui.close();
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(fid, f));
    });
    picked
}
