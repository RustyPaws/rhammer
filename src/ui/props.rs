//! Right-hand panel: object properties, texture browser / face editing, visgroups.

use crate::app::*;
use crate::editor::doc::Sel;
use crate::formats::fgd::{ClassKind, Prop};
use crate::editor::geom;
use crate::formats::vmf::{self, Entity};
use eframe::egui::{self, Color32, RichText};

enum Edit {
    Set(String, String),
    Remove(String),
}

fn parse_color255(s: &str) -> ([u8; 3], i32) {
    let n: Vec<i32> = s.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    let g = |i: usize, d: i32| n.get(i).copied().unwrap_or(d);
    ([g(0, 255).clamp(0, 255) as u8, g(1, 255).clamp(0, 255) as u8, g(2, 255).clamp(0, 255) as u8], g(3, 200))
}

/// Returns true when the value changed.
fn prop_editor(ui: &mut egui::Ui, prop: &Prop, value: &mut String, targets: &[String]) -> bool {
    let mut changed = false;
    // The FGD declares `angles(angle)` although the value is "pitch yaw roll".
    let ty = if prop.name.eq_ignore_ascii_case("angles") { "angles" } else { prop.ty.as_str() };
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
        "float" | "angle" => {
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
        "angles" | "vector" | "origin" => {
            let mut v = vmf::parse_vec3(value).unwrap_or_default();
            let mut ch = false;
            ui.horizontal(|ui| {
                for (i, l) in ["x", "y", "z"].iter().enumerate() {
                    let _ = l;
                    ch |= ui.add(egui::DragValue::new(&mut v[i]).speed(if ty == "angles" { 1.0 } else { 0.5 })).changed();
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

impl App {
    pub fn right_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, RightTab::Object, "Object");
            ui.selectable_value(&mut self.tab, RightTab::Texture, "Textures");
            ui.selectable_value(&mut self.tab, RightTab::Visgroups, "VisGroups");
        });
        ui.separator();
        match self.tab {
            RightTab::Object => self.object_tab(ui),
            RightTab::Texture => self.texture_tab(ui),
            RightTab::Visgroups => self.visgroups_tab(ui),
        }
    }

    fn object_tab(&mut self, ui: &mut egui::Ui) {
        let ents: Vec<u32> = self.sel.iter().copied().filter(|i| self.doc.entity(*i).is_some()).collect();
        let solids: Vec<u32> = self.sel.iter().copied().filter(|i| self.doc.entity(*i).is_none()).collect();
        if self.sel.is_empty() {
            ui.label(RichText::new("Nothing selected").weak());
            if ui.button("Map properties (worldspawn)…").clicked() {
                self.win.map_props = true;
            }
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if !solids.is_empty() {
                ui.label(RichText::new(format!("{} brush(es)", solids.len())).strong());
                ui.horizontal(|ui| {
                    if ui.button("Make hollow").clicked() {
                        self.hollow_selection();
                    }
                    ui.add(egui::DragValue::new(&mut self.hollow_thickness).prefix("wall ").range(1.0..=512.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Tie to entity:");
                    let cur = self.default_solid_class();
                    let mut chosen: Option<String> = None;
                    egui::ComboBox::from_id_salt("tiecls").selected_text(cur.clone()).show_ui(ui, |ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.ent_filter).hint_text("filter…"));
                        let f = self.ent_filter.to_ascii_lowercase();
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            for c in self.fgd.solid_classes() {
                                if (f.is_empty() || c.name.contains(&f)) && ui.selectable_label(false, &c.name).clicked() {
                                    chosen = Some(c.name.clone());
                                }
                            }
                        });
                    });
                    if let Some(c) = chosen {
                        self.tie_selection(&c);
                    }
                });
                ui.separator();
            }
            if let Some(&primary) = ents.first() {
                let targets: Sel = ents.iter().copied().collect();
                if ents.len() > 1 {
                    ui.label(RichText::new(format!("{} entities (editing applies to all)", ents.len())).strong());
                }
                if let Some(e) = self.doc.entity(primary).cloned() {
                    self.entity_editor(ui, &e, &targets, false);
                }
            }
        });
    }

    pub fn entity_editor(&mut self, ui: &mut egui::Ui, e: &Entity, targets: &Sel, is_world: bool) {
        let mut edits: Vec<Edit> = vec![];
        let mut new_conns: Option<Vec<(String, String)>> = None;
        let mut pick_model: Option<(String, String)> = None;
        let class_name = e.classname().to_string();
        let class = self.fgd.get(&class_name).cloned();
        let tnames = self.doc.targetnames();

        // class selector
        if !is_world {
            ui.horizontal(|ui| {
                ui.label("Class");
                let solid = !e.solids.is_empty();
                egui::ComboBox::from_id_salt("objclass").selected_text(class_name.clone()).width(ui.available_width() - 8.0).height(400.0).show_ui(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.ent_filter).hint_text("filter…"));
                    let f = self.ent_filter.to_ascii_lowercase();
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        let list: Vec<String> = self
                            .fgd
                            .names
                            .iter()
                            .filter(|n| self.fgd.get(n).map(|c| if solid { c.kind == ClassKind::Solid } else { c.kind != ClassKind::Solid }).unwrap_or(false))
                            .filter(|n| f.is_empty() || n.to_ascii_lowercase().contains(&f))
                            .cloned()
                            .collect();
                        for n in list {
                            if ui.selectable_label(n == class_name, &n).clicked() {
                                edits.push(Edit::Set("classname".into(), n));
                            }
                        }
                    });
                });
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

        egui::CollapsingHeader::new("Properties").default_open(true).show(ui, |ui| {
            let mut shown: Vec<String> = vec!["classname".into(), "spawnflags".into()];
            egui::Grid::new("props").num_columns(2).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
                if let Some(c) = &class {
                    for p in &c.props {
                        if p.name.eq_ignore_ascii_case("classname") || p.name.eq_ignore_ascii_case("spawnflags") || p.ty == "void" {
                            continue;
                        }
                        shown.push(p.name.to_ascii_lowercase());
                        let label = if p.display.is_empty() { p.name.clone() } else { p.display.clone() };
                        let r = ui.label(label);
                        if !p.help.is_empty() {
                            r.on_hover_text(format!("{}\n{}", p.name, p.help));
                        } else {
                            r.on_hover_text(&p.name);
                        }
                        let mut val = e.get(&p.name).map(|s| s.to_string()).unwrap_or_else(|| p.default.clone());
                        let orig = val.clone();
                        ui.add_enabled_ui(!p.readonly, |ui| {
                            if p.ty == "studio" {
                                // button first (right to left) so the text field takes exactly the
                                // remaining width and never pushes the panel wider
                                let w = ui.available_width() - 8.0;
                                ui.allocate_ui_with_layout(egui::vec2(w, ui.spacing().interact_size.y), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("…").on_hover_text("Browse models").clicked() {
                                        pick_model = Some((p.name.clone(), val.clone()));
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
                    ui.label(k.clone());
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
            ui.horizontal(|ui| {
                let id = ui.id().with("newkey");
                let mut kv: (String, String) = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
                ui.add(egui::TextEdit::singleline(&mut kv.0).hint_text("new key").desired_width(90.0));
                ui.add(egui::TextEdit::singleline(&mut kv.1).hint_text("value").desired_width(90.0));
                if ui.button("Add").clicked() && !kv.0.is_empty() {
                    edits.push(Edit::Set(kv.0.clone(), kv.1.clone()));
                    kv = Default::default();
                }
                ui.data_mut(|d| d.insert_temp(id, kv));
            });
        });

        // spawn flags
        if let Some(sf) = class.as_ref().and_then(|c| c.props.iter().find(|p| p.name.eq_ignore_ascii_case("spawnflags"))) {
            egui::CollapsingHeader::new("Flags").default_open(false).show(ui, |ui| {
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
            });
        }

        // model sequences / animation preview
        let model = if e.solids.is_empty() { crate::editor::doc::entity_model(e, &self.fgd).and_then(|p| self.models.get(&p).cloned().flatten()) } else { None };
        if let Some(model) = model.filter(|m| !m.sequences.is_empty()) {
            egui::CollapsingHeader::new(format!("Model ({} sequences)", model.sequences.len())).id_salt("model_anim").default_open(true).show(ui, |ui| {
                let cur = self.entity_sequence(e, &model);
                let seq = &model.sequences[cur];
                let mut pick: Option<usize> = None;
                ui.horizontal(|ui| {
                    ui.label("Sequence");
                    egui::ComboBox::from_id_salt("model_seq").selected_text(&seq.name).width(ui.available_width() - 8.0).height(400.0).show_ui(ui, |ui| {
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
                ui.horizontal(|ui| {
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
            });
        }

        // outputs
        if !is_world {
            let title = format!("Outputs ({})", e.connections.len());
            egui::CollapsingHeader::new(title).id_salt("outputs").default_open(false).show(ui, |ui| {
                let mut conns = e.connections.clone();
                let mut changed = false;
                let mut remove: Option<usize> = None;
                for (i, (out, val)) in conns.iter_mut().enumerate() {
                    // newer branches (Portal 2, L4D2, CS:GO) separate fields with ESC instead of ','
                    let sep = if val.contains('\x1b') { '\x1b' } else { ',' };
                    let mut parts: Vec<String> = val.splitn(5, sep).map(|s| s.to_string()).collect();
                    while parts.len() < 5 {
                        parts.push(if parts.len() == 3 { "0".into() } else if parts.len() == 4 { "-1".into() } else { String::new() });
                    }
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("When");
                            let outs: Vec<String> = class.as_ref().map(|c| c.outputs.iter().map(|o| o.name.clone()).collect()).unwrap_or_default();
                            changed |= ui.add(egui::TextEdit::singleline(out).desired_width(110.0)).changed();
                            ui.menu_button("v", |ui| {
                                for o in outs {
                                    if ui.button(&o).clicked() {
                                        *out = o;
                                        changed = true;
                                        ui.close();
                                    }
                                }
                            });
                            if ui.small_button("X").clicked() {
                                remove = Some(i);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("fires");
                            changed |= ui.add(egui::TextEdit::singleline(&mut parts[0]).hint_text("target").desired_width(100.0)).changed();
                            ui.menu_button("v", |ui| {
                                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                                    for t in &tnames {
                                        if ui.button(t).clicked() {
                                            parts[0] = t.clone();
                                            changed = true;
                                            ui.close();
                                        }
                                    }
                                });
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("input");
                            changed |= ui.add(egui::TextEdit::singleline(&mut parts[1]).desired_width(100.0)).changed();
                            // inputs of the target's class
                            let target_class = self.doc.map.entities.iter().find(|x| x.get("targetname") == Some(parts[0].as_str())).map(|x| x.classname().to_string());
                            let inputs: Vec<String> = target_class
                                .and_then(|c| self.fgd.get(&c))
                                .map(|c| c.inputs.iter().map(|i| i.name.clone()).collect())
                                .unwrap_or_else(|| vec!["Kill".into(), "Enable".into(), "Disable".into(), "Toggle".into(), "Trigger".into(), "Open".into(), "Close".into()]);
                            ui.menu_button("v", |ui| {
                                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                                    for inp in inputs {
                                        if ui.button(&inp).clicked() {
                                            parts[1] = inp;
                                            changed = true;
                                            ui.close();
                                        }
                                    }
                                });
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("param");
                            changed |= ui.add(egui::TextEdit::singleline(&mut parts[2]).desired_width(80.0)).changed();
                            let mut delay: f64 = parts[3].parse().unwrap_or(0.0);
                            if ui.add(egui::DragValue::new(&mut delay).prefix("delay ").speed(0.05).range(0.0..=3600.0)).changed() {
                                parts[3] = vmf::fmt(delay);
                                changed = true;
                            }
                            let mut once = parts[4] == "1";
                            if ui.checkbox(&mut once, "once").changed() {
                                parts[4] = if once { "1".into() } else { "-1".into() };
                                changed = true;
                            }
                        });
                    });
                    *val = parts.join(&sep.to_string());
                }
                if let Some(i) = remove {
                    conns.remove(i);
                    changed = true;
                }
                if ui.button("+ Add output").clicked() {
                    let esc = self.doc.map.entities.iter().flat_map(|x| &x.connections).any(|(_, v)| v.contains('\x1b'));
                    let v = if esc { "\x1bTrigger\x1b\x1b0\x1b-1" } else { ",Trigger,,0,-1" };
                    conns.push(("OnTrigger".into(), v.into()));
                    changed = true;
                }
                if changed {
                    new_conns = Some(conns);
                }
            });
        }

        // IO documentation
        if let Some(c) = &class {
            if !c.inputs.is_empty() {
                egui::CollapsingHeader::new(format!("Inputs ({})", c.inputs.len())).default_open(false).show(ui, |ui| {
                    for i in &c.inputs {
                        ui.label(RichText::new(format!("{}({})", i.name, i.ty)).strong());
                        if !i.help.is_empty() {
                            ui.label(RichText::new(&i.help).small().weak());
                        }
                    }
                });
            }
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
        let now = std::time::Instant::now();
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

    // ---- textures ------------------------------------------------------------------------

    pub fn load_face_edit(&mut self) {
        let Some(first) = self.faces.iter().next().copied() else { return };
        for s in self.doc.map.world.solids.iter().chain(self.doc.map.entities.iter().flat_map(|e| e.solids.iter())) {
            for sd in &s.sides {
                if sd.id == first {
                    self.face_edit = FaceEdit {
                        uscale: sd.uaxis.scale,
                        vscale: sd.vaxis.scale,
                        ushift: sd.uaxis.shift,
                        vshift: sd.vaxis.shift,
                        rotation: sd.rotation,
                        lightmap: sd.lightmap,
                    };
                    self.cur_mat = sd.material.clone();
                    return;
                }
            }
        }
    }

    fn for_each_face(&mut self, mut f: impl FnMut(&mut crate::formats::vmf::Side)) {
        let faces = self.faces.clone();
        for s in self.doc.map.world.solids.iter_mut().chain(self.doc.map.entities.iter_mut().flat_map(|e| e.solids.iter_mut())) {
            for sd in &mut s.sides {
                if faces.contains(&sd.id) {
                    f(sd);
                }
            }
        }
        self.doc.touch();
    }

    pub fn apply_material_to_faces(&mut self) {
        if self.faces.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let m = self.cur_mat.clone();
        self.for_each_face(|sd| sd.material = m.clone());
    }

    pub fn apply_material_to_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let m = self.cur_mat.clone();
        let sel = self.sel.clone();
        for s in self.doc.map.world.solids.iter_mut() {
            if sel.contains(&s.id) {
                s.sides.iter_mut().for_each(|sd| sd.material = m.clone());
            }
        }
        for e in self.doc.map.entities.iter_mut() {
            if sel.contains(&e.id) {
                e.solids.iter_mut().flat_map(|s| s.sides.iter_mut()).for_each(|sd| sd.material = m.clone());
            }
        }
        self.doc.touch();
    }

    fn texture_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Current material").strong());
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.cur_mat).desired_width(ui.available_width() - 8.0));
        });
        ui.horizontal(|ui| {
            if ui.button("Apply to faces").clicked() {
                self.apply_material_to_faces();
            }
            if ui.button("Apply to selected objects").clicked() {
                self.apply_material_to_selection();
            }
        });
        if !self.faces.is_empty() {
            ui.separator();
            ui.label(RichText::new(format!("{} face(s) selected", self.faces.len())).strong());
            let mut fe = self.face_edit.clone();
            let mut changed = false;
            egui::Grid::new("faceedit").num_columns(3).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label("Scale");
                changed |= ui.add(egui::DragValue::new(&mut fe.uscale).speed(0.01).range(0.001..=64.0)).changed();
                changed |= ui.add(egui::DragValue::new(&mut fe.vscale).speed(0.01).range(0.001..=64.0)).changed();
                ui.end_row();
                ui.label("Shift");
                changed |= ui.add(egui::DragValue::new(&mut fe.ushift).speed(1.0)).changed();
                changed |= ui.add(egui::DragValue::new(&mut fe.vshift).speed(1.0)).changed();
                ui.end_row();
                ui.label("Rotation");
                changed |= ui.add(egui::DragValue::new(&mut fe.rotation).speed(1.0)).changed();
                ui.end_row();
                ui.label("Lightmap");
                changed |= ui.add(egui::DragValue::new(&mut fe.lightmap).range(1..=1024)).changed();
                ui.end_row();
            });
            if changed {
                self.face_edit = fe.clone();
                self.doc.checkpoint();
                self.for_each_face(|sd| {
                    sd.uaxis.scale = fe.uscale;
                    sd.vaxis.scale = fe.vscale;
                    sd.uaxis.shift = fe.ushift;
                    sd.vaxis.shift = fe.vshift;
                    sd.rotation = fe.rotation;
                    sd.lightmap = fe.lightmap;
                });
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("Align to world").clicked() {
                    self.doc.checkpoint();
                    let fe = self.face_edit.clone();
                    self.for_each_face(|sd| {
                        let n = geom::Plane::from_points(&sd.plane).map(|p| p.n).unwrap_or(glam::DVec3::Z);
                        let (u, v) = geom::default_axes(n, fe.uscale);
                        sd.uaxis = u;
                        sd.vaxis = v;
                        sd.vaxis.scale = fe.vscale;
                        sd.rotation = 0.0;
                    });
                }
                if ui.button("Align to face").clicked() {
                    self.doc.checkpoint();
                    let fe = self.face_edit.clone();
                    self.for_each_face(|sd| {
                        let n = geom::Plane::from_points(&sd.plane).map(|p| p.n).unwrap_or(glam::DVec3::Z);
                        let (u, v) = geom::basis(n);
                        sd.uaxis = vmf::TexAxis { vec: u, shift: 0.0, scale: fe.uscale };
                        sd.vaxis = vmf::TexAxis { vec: -v, shift: 0.0, scale: fe.vscale };
                        sd.rotation = 0.0;
                    });
                }
            });
        }
        ui.separator();
        self.texture_grid(ui);
    }

    /// Scrollable thumbnail grid. Click to choose the current material.
    pub fn texture_grid(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::TextEdit::singleline(&mut self.tex_filter).hint_text("filter materials…").desired_width(f32::INFINITY));
        let f = self.tex_filter.to_ascii_lowercase();
        let list: Vec<String> = self
            .mats
            .all
            .iter()
            .filter(|m| f.is_empty() || f.split_whitespace().all(|w| m.contains(w)))
            .cloned()
            .collect();
        ui.label(RichText::new(format!("{} materials", list.len())).small().weak());
        let cell = 76.0;
        let cols = ((ui.available_width() / (cell + 6.0)).floor() as usize).max(1);
        let rows = list.len().div_ceil(cols);
        let mut loads = 6;
        let mut choose: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, cell + 22.0, rows, |ui, range| {
            for r in range {
                ui.horizontal(|ui| {
                    for c in 0..cols {
                        let Some(name) = list.get(r * cols + c) else { break };
                        ui.vertical(|ui| {
                            ui.set_width(cell);
                            let tex = self.thumb_tex.get(name).cloned().or_else(|| {
                                if loads == 0 {
                                    return None;
                                }
                                loads -= 1;
                                let t = self.mats.thumb(name);
                                let handle = t.map(|(w, h, mut rgba)| {
                                    // texture alpha holds masks, not transparency
                                    rgba.chunks_exact_mut(4).for_each(|px| px[3] = 255);
                                    let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                                    ui.ctx().load_texture(format!("thumb:{name}"), img, egui::TextureOptions::LINEAR)
                                });
                                if let Some(h) = &handle {
                                    self.thumb_tex.insert(name.clone(), h.clone());
                                } else {
                                    // remember failures as a 1x1 placeholder to avoid retrying every frame
                                    let img = egui::ColorImage::filled([1, 1], Color32::from_gray(40));
                                    let h = ui.ctx().load_texture(format!("thumb:{name}"), img, egui::TextureOptions::LINEAR);
                                    self.thumb_tex.insert(name.clone(), h.clone());
                                    return Some(h);
                                }
                                handle
                            });
                            let selected = self.cur_mat.eq_ignore_ascii_case(name);
                            let resp = match &tex {
                                Some(t) => ui.add(egui::Button::image(egui::Image::new(t).fit_to_exact_size(egui::Vec2::splat(cell - 8.0))).selected(selected)),
                                None => {
                                    ui.ctx().request_repaint();
                                    ui.add_sized([cell, cell - 4.0], egui::Button::new("…").selected(selected))
                                }
                            };
                            let short = name.rsplit('/').next().unwrap_or(name);
                            ui.label(RichText::new(short).small());
                            if resp.on_hover_text(name).clicked() {
                                choose = Some(name.clone());
                            }
                        });
                    }
                });
            }
        });
        if let Some(c) = choose {
            self.cur_mat = c;
        }
    }

    // ---- visgroups -----------------------------------------------------------------------

    fn visgroups_tab(&mut self, ui: &mut egui::Ui) {
        let groups = self.doc.visgroups();
        if groups.is_empty() {
            ui.label(RichText::new("No visgroups").weak());
        }
        let mut toggle: Option<(u32, bool)> = None;
        let mut select: Option<u32> = None;
        let mut assign: Option<(u32, bool)> = None;
        let mut delete: Option<u32> = None;
        for g in &groups {
            ui.horizontal(|ui| {
                ui.add_space(g.depth as f32 * 14.0);
                let mut shown = self.doc.visgroup_shown(g.id);
                if ui.checkbox(&mut shown, "").changed() {
                    toggle = Some((g.id, shown));
                }
                ui.label(&g.name);
                if ui.small_button("sel").on_hover_text("select members").clicked() {
                    select = Some(g.id);
                }
                if ui.small_button("+").on_hover_text("add selection to group").clicked() {
                    assign = Some((g.id, true));
                }
                if ui.small_button("−").on_hover_text("remove selection from group").clicked() {
                    assign = Some((g.id, false));
                }
                if ui.small_button("🗑").on_hover_text("delete group (objects are kept)").clicked() {
                    delete = Some(g.id);
                }
            });
        }
        if let Some(id) = delete {
            self.doc.checkpoint();
            self.doc.delete_visgroup(id);
            self.bump_sel();
            return;
        }
        if let Some((id, shown)) = toggle {
            self.doc.checkpoint();
            self.doc.set_visgroup_shown(id, shown);
            self.sel.retain(|i| !self.doc.is_hidden(*i));
            self.bump_sel();
        }
        if let Some(id) = select {
            let m: Sel = self.doc.visgroup_members(id).into_iter().collect();
            self.set_sel(m);
        }
        if let Some((id, add)) = assign {
            self.doc.checkpoint();
            let s = self.sel.clone();
            self.doc.assign_visgroup(&s, id, add);
            self.doc.touch();
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_visgroup).hint_text("new group name"));
            if ui.button("Add group").clicked() && !self.new_visgroup.trim().is_empty() {
                self.doc.checkpoint();
                let n = self.new_visgroup.trim().to_string();
                self.doc.add_visgroup(&n);
                self.new_visgroup.clear();
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Show all").clicked() {
                self.doc.checkpoint();
                for g in &groups {
                    self.doc.set_visgroup_shown(g.id, true);
                }
                // also reveal anything hidden outside groups
                self.doc.touch();
            }
        });
    }
}
