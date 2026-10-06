//! The entity property editor: one tab of the Object Properties window at a time.

use crate::editor::doc::Sel;
use crate::formats::fgd::ClassKind;
use crate::formats::vmf::{self, Entity};
use eframe::egui::{self, Color32, RichText};
use super::*;
use super::widgets::{autocomplete_edit, filter_combo, pick_list_button, prop_editor};

pub(crate) enum Edit {
    Set(String, String),
    Remove(String),
}

impl App {
    pub fn entity_editor(&mut self, ui: &mut egui::Ui, e: &Entity, targets: &Sel, is_world: bool, tab: ObjTab) {
        let mut edits: Vec<Edit> = vec![];
        let mut new_conns: Option<Vec<vmf::Connection>> = None;
        let mut pick_model: Option<(String, String)> = None;
        let mut pick_sound: Option<(String, String)> = None;
        let class_name = e.classname().to_string();
        let class = self.fgd.get(&class_name).cloned();
        let tnames = self.doc.targetnames();

        let mut goto: Option<u32> = None;

        // class selector
        if !is_world && tab == ObjTab::Properties {
            ui.horizontal(|ui| {
                ui.label("Class");
                let solid = !e.solids.is_empty();
                // brush entities can only become other brush entities, point entities other point ones
                let fgd = &self.fgd;
                let classes = fgd.names.iter().map(String::as_str).filter(|n| fgd.get(n).is_some_and(|c| (c.kind == ClassKind::Solid) == solid));
                let w = ui.available_width() - 8.0;
                if let Some(n) = filter_combo(ui, "objclass", &class_name, Some(w), &mut self.ent_filter, classes) {
                    edits.push(Edit::Set("classname".into(), n));
                }
            });
            if let Some(c) = &class {
                if !c.desc.is_empty() {
                    ui.label(RichText::new(&c.desc).small().weak());
                }
            } else {
                ui.colored_label(Color32::from_rgb(230, 170, 60), "Class not found in FGD");
            }
            ui.separator();
        }

        if tab == ObjTab::Properties {
            let mut shown: Vec<String> = vec!["classname".into(), "spawnflags".into()];
            // long FGD names are cut short (the full name is in the tooltip) so values keep their room
            let full_w = ui.available_width();
            let label_w = (full_w * self.settings.ui.prop_label_frac).clamp(60.0, (full_w - 60.0).max(60.0));
            // the value column is capped to the visible width: Grid otherwise keeps last frame's
            // widest cell, and one wide editor would stretch every text field past the edge
            let value_w = (ui.available_width() - label_w - 8.0).max(60.0);
            let grid = egui::Grid::new("props").num_columns(2).spacing([8.0, 4.0]).max_col_width(label_w.max(value_w)).striped(true).show(ui, |ui| {
                if let Some(c) = &class {
                    for p in &c.props {
                        if p.name.eq_ignore_ascii_case("classname") || p.name.eq_ignore_ascii_case("spawnflags") || p.ty == "void" {
                            continue;
                        }
                        shown.push(p.name.to_ascii_lowercase());
                        let label = if p.display.is_empty() { p.name.clone() } else { p.display.clone() };
                        let r = key_label(ui, label, label_w);
                        if !p.help.is_empty() {
                            r.on_hover_text(format!("{}\n{}", p.name, p.help));
                        } else {
                            r.on_hover_text(&p.name);
                        }
                        let mut val = e.get(&p.name).map(|s| s.to_string()).unwrap_or_else(|| p.default.clone());
                        let orig = val.clone();
                        ui.add_enabled_ui(!p.readonly, |ui| {
                            if p.ty == "studio" || p.ty == "sound" {
                                // button first (right to left) so the text field takes exactly the
                                // remaining width and never pushes the panel wider
                                let w = ui.available_width() - 8.0;
                                ui.allocate_ui_with_layout(egui::vec2(w, ui.spacing().interact_size.y), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let sound = p.ty == "sound";
                                    if ui.button("…").on_hover_text(if sound { "Browse sounds" } else { "Browse models" }).clicked() {
                                        let target = Some((p.name.clone(), val.clone()));
                                        if sound { pick_sound = target } else { pick_model = target }
                                    }
                                    let tw = (ui.available_width() - 2.0).max(40.0);
                                    if ui.add(egui::TextEdit::singleline(&mut val).desired_width(tw)).changed() && val != orig {
                                        edits.push(Edit::Set(p.name.clone(), val.clone()));
                                    }
                                });
                            } else if prop_editor(ui, p, &mut val, &tnames) && val != orig {
                                edits.push(Edit::Set(p.name.clone(), val.clone()));
                            }
                        });
                        ui.end_row();
                    }
                }
                // keys unknown to the FGD
                for (k, v) in &e.props {
                    let lk = k.to_ascii_lowercase();
                    if shown.contains(&lk) || lk == "id" {
                        continue;
                    }
                    key_label(ui, k.clone(), label_w).on_hover_text(k);
                    let mut val = v.clone();
                    ui.horizontal(|ui| {
                        if ui.add(egui::TextEdit::singleline(&mut val).desired_width(ui.available_width() - 28.0)).changed() {
                            edits.push(Edit::Set(k.clone(), val.clone()));
                        }
                        if ui.small_button("X").on_hover_text("remove key").clicked() {
                            edits.push(Edit::Remove(k.clone()));
                        }
                    });
                    ui.end_row();
                }
            });
            // draggable divider between the key and value columns, remembered in the layout
            let gr = grid.response.rect;
            let bar = egui::Rect::from_min_max(egui::pos2(gr.left() + label_w + 1.0, gr.top()), egui::pos2(gr.left() + label_w + 7.0, gr.bottom()));
            let handle = ui.interact(bar, ui.id().with("prop_divider"), egui::Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            if let Some(p) = handle.interact_pointer_pos().filter(|_| handle.dragged()) {
                self.settings.ui.prop_label_frac = ((p.x - gr.left() - 3.0) / full_w).clamp(0.15, 0.75);
            }
            if handle.hovered() || handle.dragged() {
                ui.painter().vline(bar.center().x, gr.y_range(), egui::Stroke::new(1.5, ui.visuals().widgets.hovered.fg_stroke.color));
            }
            ui.horizontal(|ui| {
                let id = ui.id().with("newkey");
                let mut kv: (String, String) = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
                let mut add = false;
                trailing(ui, |ui| {
                    add = ui.button("Add").clicked();
                    fill_rest(ui, |ui| {
                        let w = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(40.0);
                        ui.add(egui::TextEdit::singleline(&mut kv.0).hint_text("new key").desired_width(w));
                        ui.add(egui::TextEdit::singleline(&mut kv.1).hint_text("value").desired_width(w));
                    });
                });
                if add && !kv.0.is_empty() {
                    edits.push(Edit::Set(kv.0.clone(), kv.1.clone()));
                    kv = Default::default();
                }
                ui.data_mut(|d| d.insert_temp(id, kv));
            });
        }

        // spawn flags
        let has_flags = class.as_ref().is_some_and(|c| c.props.iter().any(|p| p.name.eq_ignore_ascii_case("spawnflags")));
        if tab == ObjTab::Flags && !has_flags {
            ui.label(RichText::new("This class has no flags.").weak());
        }
        if let Some(sf) = class.as_ref().and_then(|c| c.props.iter().find(|p| p.name.eq_ignore_ascii_case("spawnflags"))) {
            if tab == ObjTab::Flags {
                let mut flags: i64 = e.get("spawnflags").and_then(|s| s.parse().ok()).unwrap_or_else(|| sf.default.parse().unwrap_or(0));
                let before = flags;
                for c in &sf.choices {
                    let bit: i64 = c.value.parse().unwrap_or(0);
                    let mut on = flags & bit != 0;
                    if ui.checkbox(&mut on, &c.label).changed() {
                        if on { flags |= bit } else { flags &= !bit }
                    }
                }
                if flags != before {
                    edits.push(Edit::Set("spawnflags".into(), flags.to_string()));
                }
            }
        }

        // model sequences / animation preview
        let model = if e.solids.is_empty() { crate::editor::doc::entity_model(e, &self.fgd).and_then(|p| self.models.get(&p).cloned().flatten()) } else { None };
        if let Some(model) = model.filter(|m| !m.sequences.is_empty()) {
            if tab == ObjTab::Properties {
                ui.separator();
                ui.label(RichText::new(format!("Model ({} sequences)", model.sequences.len())).strong());
                let cur = self.entity_sequence(e, &model);
                let seq = &model.sequences[cur];
                let mut pick: Option<usize> = None;
                ui.horizontal(|ui| {
                    ui.label("Sequence");
                    egui::ComboBox::from_id_salt("model_seq").selected_text(&seq.name).width(ui.available_width() - 8.0).height(400.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show_ui(ui, |ui| {
                        let id = ui.id().with("seqfilter");
                        let mut f: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
                        ui.add(egui::TextEdit::singleline(&mut f).hint_text("filter…"));
                        let fl = f.to_ascii_lowercase();
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            for (i, s) in model.sequences.iter().enumerate() {
                                if !fl.is_empty() && !s.name.to_ascii_lowercase().contains(&fl) && !s.activity.to_ascii_lowercase().contains(&fl) {
                                    continue;
                                }
                                let label = format!("{}  ({}f)", s.name, s.frames);
                                let r = ui.selectable_label(i == cur, label);
                                let r = if s.activity.is_empty() { r } else { r.on_hover_text(&s.activity) };
                                if r.clicked() {
                                    pick = Some(i);
                                    ui.close();
                                }
                            }
                        });
                        ui.data_mut(|d| d.insert_temp(id, f));
                    });
                });
                if let Some(i) = pick {
                    for id in targets {
                        self.anim_preview.insert(*id, i);
                    }
                    self.anim_time = 0.0;
                    self.anim_stamp += 1;
                }
                let dur = seq.duration();
                ui.horizontal_wrapped(|ui| {
                    if ui.button(if self.anim_play { "⏸" } else { "▶" }).on_hover_text("Play / pause model animations").clicked() {
                        self.anim_play = !self.anim_play;
                    }
                    if ui.button("⏮").on_hover_text("Restart").clicked() {
                        self.anim_time = 0.0;
                        self.anim_stamp += 1;
                    }
                    if dur > 0.0 {
                        let mut t = self.anim_time.rem_euclid(dur);
                        if ui.add(egui::Slider::new(&mut t, 0.0..=dur).show_value(false)).changed() {
                            self.anim_time = t;
                            self.anim_play = false;
                            self.anim_stamp += 1;
                        }
                        ui.label(format!("{:.0}/{}", seq.frame_at(self.anim_time), seq.frames - 1));
                    }
                });
                let mut info = format!("{} frames @ {} fps", seq.frames, seq.fps);
                if seq.looping {
                    info.push_str(", looping");
                }
                if !seq.activity.is_empty() {
                    info.push_str(&format!(", {}", seq.activity));
                }
                ui.label(RichText::new(info).small().weak());
                ui.horizontal(|ui| {
                    let has_default = class.as_ref().map(|c| c.props.iter().any(|p| p.name.eq_ignore_ascii_case("DefaultAnim"))).unwrap_or(false) || e.get("DefaultAnim").is_some();
                    let is_default = e.get("DefaultAnim").map(|d| d.eq_ignore_ascii_case(&seq.name)).unwrap_or(false);
                    if has_default && ui.add_enabled(!is_default, egui::Button::new("Set as DefaultAnim")).clicked() {
                        edits.push(Edit::Set("DefaultAnim".into(), seq.name.clone()));
                    }
                    if self.anim_preview.contains_key(&e.id) && ui.button("Reset preview").on_hover_text("Show the sequence the map uses").clicked() {
                        for id in targets {
                            self.anim_preview.remove(id);
                        }
                        self.anim_stamp += 1;
                    }
                });
            }
        }

        // outputs
        if !is_world {
            if tab == ObjTab::Outputs {
                let mut conns = e.connections.clone();
                let mut changed = false;
                let mut remove: Option<usize> = None;
                for (i, c) in conns.iter_mut().enumerate() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("When");
                            let outs: Vec<String> = class.as_ref().map(|c| c.outputs.iter().map(|o| o.name.clone()).collect()).unwrap_or_default();
                            trailing(ui, |ui| {
                                if ui.small_button("X").on_hover_text("remove output").clicked() {
                                    remove = Some(i);
                                }
                                if let Some(o) = pick_list_button(ui, &format!("out_{i}"), &outs) {
                                    c.output = o;
                                    changed = true;
                                }
                                fill_rest(ui, |ui| changed |= autocomplete_edit(ui, &format!("out_when_{i}"), &mut c.output, "output", &outs, ui.available_width()));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("fires");
                            trailing(ui, |ui| {
                                if let Some(t) = pick_list_button(ui, &format!("tgt_{i}"), &tnames) {
                                    c.target = t;
                                    changed = true;
                                }
                                fill_rest(ui, |ui| changed |= autocomplete_edit(ui, &format!("out_target_{i}"), &mut c.target, "target", &tnames, ui.available_width()));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("input");
                            // inputs of the target's class
                            let target_class = self.doc.map.entities.iter().find(|x| x.get("targetname") == Some(c.target.as_str())).map(|x| x.classname().to_string());
                            let inputs: Vec<String> = target_class
                                .and_then(|c| self.fgd.get(&c))
                                .map(|c| c.inputs.iter().map(|i| i.name.clone()).collect())
                                .unwrap_or_else(|| vec!["Kill".into(), "Enable".into(), "Disable".into(), "Toggle".into(), "Trigger".into(), "Open".into(), "Close".into()]);
                            trailing(ui, |ui| {
                                if let Some(inp) = pick_list_button(ui, &format!("inp_{i}"), &inputs) {
                                    c.input = inp;
                                    changed = true;
                                }
                                fill_rest(ui, |ui| changed |= autocomplete_edit(ui, &format!("out_input_{i}"), &mut c.input, "input", &inputs, ui.available_width()));
                            });
                        });
                        // wraps on a narrow panel instead of cutting "once" off
                        ui.horizontal_wrapped(|ui| {
                            ui.label("param");
                            changed |= ui.add(egui::TextEdit::singleline(&mut c.param).desired_width((ui.available_width() - 8.0).clamp(40.0, 120.0))).changed();
                            changed |= ui.add(egui::DragValue::new(&mut c.delay).prefix("delay ").speed(0.05).range(0.0..=3600.0)).changed();
                            // -1 = unlimited, 1 = once, N = exactly N times
                            let prev = c.times;
                            let times = egui::DragValue::new(&mut c.times)
                                .range(-1..=9999)
                                .speed(0.2)
                                .prefix("times ")
                                .custom_formatter(|n, _| if n < 1.0 { "unlimited".to_string() } else { format!("{n:.0}") })
                                .custom_parser(|t| t.trim().parse::<f64>().ok().or_else(|| t.trim().to_ascii_lowercase().starts_with("un").then_some(-1.0)));
                            if ui.add(times).on_hover_text("times to fire: -1 = unlimited, 1 = once").changed() {
                                // 0 is not a valid count: step over it in the direction of the drag
                                if c.times == 0 {
                                    c.times = if prev < 0 { 1 } else { -1 };
                                }
                                changed = true;
                            }
                        });
                    });
                }
                if let Some(i) = remove {
                    conns.remove(i);
                    changed = true;
                }
                if ui.button("+ Add output").clicked() {
                    let esc = self.doc.map.entities.iter().flat_map(|x| &x.connections).any(|c| c.sep == '\x1b');
                    conns.push(vmf::Connection::new(if esc { '\x1b' } else { ',' }));
                    changed = true;
                }
                if changed {
                    new_conns = Some(conns);
                }
            }
        }

        if tab == ObjTab::Inputs && !is_world {
            goto = self.incoming_ui(ui, e);
            if let Some(c) = class.as_ref().filter(|c| !c.inputs.is_empty()) {
                ui.separator();
                egui::CollapsingHeader::new(format!("Inputs of {} ({})", c.name, c.inputs.len())).id_salt("input_docs").default_open(false).show(ui, |ui| {
                    for i in &c.inputs {
                        ui.label(RichText::new(format!("{}({})", i.name, i.ty)).strong());
                        if !i.help.is_empty() {
                            ui.label(RichText::new(&i.help).small().weak());
                        }
                    }
                });
            }
        }

        if let Some(id) = goto {
            self.set_sel([id].into_iter().collect());
            return;
        }

        if let Some((key, cur)) = pick_sound {
            let mut ids = targets.clone();
            if is_world {
                ids.insert(self.doc.map.world.id);
            }
            self.pick_sound(ids, &key, &cur);
        }

        if let Some((key, cur)) = pick_model {
            let mut ids = targets.clone();
            if is_world {
                ids.insert(self.doc.map.world.id);
            }
            self.pick_model(ids, &key, &cur);
        }

        // apply edits
        if edits.is_empty() && new_conns.is_none() {
            return;
        }
        let now = web_time::Instant::now();
        let key = edits.first().map(|e| match e {
            Edit::Set(k, _) | Edit::Remove(k) => k.clone(),
        });
        let coalesce = match (&self.last_prop_edit, &key) {
            (Some((k, t)), Some(nk)) => k == nk && now.duration_since(*t).as_secs_f32() < 0.8,
            _ => false,
        };
        if !coalesce {
            self.doc.checkpoint();
        }
        self.last_prop_edit = key.map(|k| (k, now)).or(Some(("outputs".into(), now)));
        let mut ids = targets.clone();
        if is_world {
            ids.insert(self.doc.map.world.id);
        }
        for ed in edits {
            match ed {
                Edit::Set(k, v) => {
                    self.doc.set_prop(&ids, &k, &v);
                    if k.eq_ignore_ascii_case("classname") {
                        // refresh the editor color for the new class
                        if let Some(c) = self.fgd.get(&v).and_then(|c| c.color) {
                            for id in targets {
                                if let Some(en) = self.doc.entity_mut(*id) {
                                    crate::editor::doc::Doc::set_editor(&mut en.editor, "color", &format!("{} {} {}", c[0], c[1], c[2]));
                                }
                            }
                        }
                    }
                }
                Edit::Remove(k) => self.doc.set_prop(&ids, &k, ""),
            }
        }
        if let Some(c) = new_conns {
            for id in targets {
                if let Some(en) = self.doc.entity_mut(*id) {
                    en.connections = c.clone();
                }
            }
        }
        self.doc.touch();
    }

    /// The Inputs tab: every output in the map that targets this entity. Returns the entity whose
    /// "go to" button was clicked.
    fn incoming_ui(&self, ui: &mut egui::Ui, e: &Entity) -> Option<u32> {
        let Some(name) = e.get("targetname").filter(|n| !n.is_empty()) else {
            ui.label(RichText::new("This entity has no name, so no output can target it.").weak());
            return None;
        };
        let incoming: Vec<(&Entity, &vmf::Connection)> =
            self.doc.map.entities.iter().flat_map(|src| src.connections.iter().filter(|c| crate::ui::io_graph::glob(&c.target, name)).map(move |c| (src, c))).collect();
        if incoming.is_empty() {
            ui.label(RichText::new(format!("No output targets \"{name}\".")).weak());
            return None;
        }
        let mut goto = None;
        for (src, c) in incoming {
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                trailing(ui, |ui| {
                    if src.id != e.id && ui.small_button("go to").on_hover_text("select the entity that fires this").clicked() {
                        goto = Some(src.id);
                    }
                    fill_rest(ui, |ui| {
                        let who = src.get("targetname").filter(|n| !n.is_empty()).map_or_else(|| format!("{} #{}", src.classname(), src.id), |n| format!("{n} ({})", src.classname()));
                        ui.add(egui::Label::new(RichText::new(who).strong()).truncate());
                    });
                });
                let mut what = format!("{} \u{2192} {}", c.output, c.input);
                if !c.param.is_empty() {
                    what += &format!("({})", c.param);
                }
                if c.delay > 0.0 {
                    what += &format!(" after {}s", vmf::fmt(c.delay));
                }
                match c.times {
                    1 => what += " once",
                    n if n > 1 => what += &format!(" \u{d7}{n}"),
                    _ => {}
                }
                ui.add(egui::Label::new(what).wrap());
            });
        }
        goto
    }
}

/// A property name in the key column, cut to `width` so long names don't push the values out.
fn key_label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, width: f32) -> egui::Response {
    let h = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(egui::vec2(width, h), egui::Layout::left_to_right(egui::Align::Center), |ui| ui.add(egui::Label::new(text).truncate())).inner
}
