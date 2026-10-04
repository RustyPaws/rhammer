//! Right-hand panel: object properties, texture browser / face editing, visgroups.

use crate::app::*;
use crate::editor::doc::Sel;
use eframe::egui::{self, RichText};


mod angles;
mod entity;
mod textures;
mod visgroups;
mod widgets;

impl App {
    pub(crate) fn object_tab(&mut self, ui: &mut egui::Ui) {
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
                    egui::ComboBox::from_id_salt("tiecls").selected_text(cur.clone()).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show_ui(ui, |ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.ent_filter).hint_text("filter…"));
                        let f = self.ent_filter.to_ascii_lowercase();
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            for c in self.fgd.solid_classes() {
                                if (f.is_empty() || c.name.to_ascii_lowercase().contains(&f)) && ui.selectable_label(false, &c.name).clicked() {
                                    chosen = Some(c.name.clone());
                                    ui.close();
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
}
