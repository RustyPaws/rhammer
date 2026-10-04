//! The Object Properties window and the right-hand panel tabs (textures / face editing, visgroups).

use crate::app::*;
use crate::editor::doc::Sel;
use eframe::egui::{self, RichText};


mod angles;
mod entity;
mod textures;
mod visgroups;
pub(crate) mod widgets;

/// Tabs of the Object Properties window, as in Hammer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObjTab {
    #[default]
    Properties,
    Outputs,
    Inputs,
    Flags,
}

impl ObjTab {
    pub const ALL: [ObjTab; 4] = [ObjTab::Properties, ObjTab::Outputs, ObjTab::Inputs, ObjTab::Flags];

    pub fn name(self) -> &'static str {
        match self {
            ObjTab::Properties => "Properties",
            ObjTab::Outputs => "Outputs",
            ObjTab::Inputs => "Inputs",
            ObjTab::Flags => "Flags",
        }
    }
}

/// Scrolls both ways. Widgets still size themselves to the visible width, and whatever can't
/// shrink any further stays reachable through the horizontal scrollbar.
pub(crate) fn panel_scroll<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, add).inner
}

/// Lays a row out right to left: add the trailing widgets first, then call [`fill_rest`],
/// which gets exactly the width that is left.
pub(crate) fn trailing<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    // the outer row keeps it one line high: on its own, right_to_left in a vertical layout
    // would centre itself in all the height that is left
    ui.horizontal(|ui| ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add).inner).inner
}

/// Inside [`trailing`]: the remaining width, left to right.
pub(crate) fn fill_rest<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), add).inner
}

impl App {
    /// Contents of the Object Properties window: brush tools, then the tabs of the first
    /// selected entity (edits apply to every selected entity).
    pub(crate) fn object_props_ui(&mut self, ui: &mut egui::Ui) {
        let ents: Vec<u32> = self.sel.iter().copied().filter(|i| self.doc.entity(*i).is_some()).collect();
        let solids: Vec<u32> = self.sel.iter().copied().filter(|i| self.doc.entity(*i).is_none()).collect();
        if self.sel.is_empty() {
            ui.label(RichText::new("Nothing selected").weak());
            if ui.button("Map properties (worldspawn)…").clicked() {
                self.win.map_props = true;
            }
            return;
        }
        let primary = ents.first().and_then(|id| self.doc.entity(*id)).cloned();
        if let Some(e) = &primary {
            ui.horizontal_wrapped(|ui| {
                for t in ObjTab::ALL {
                    let label = match t {
                        ObjTab::Outputs if !e.connections.is_empty() => format!("{} ({})", t.name(), e.connections.len()),
                        _ => t.name().to_string(),
                    };
                    ui.selectable_value(&mut self.obj_tab, t, label);
                }
            });
            ui.separator();
        }
        let tab = if primary.is_some() { self.obj_tab } else { ObjTab::Properties };
        panel_scroll(ui, |ui| {
            if !solids.is_empty() && tab == ObjTab::Properties {
                ui.label(RichText::new(format!("{} brush(es)", solids.len())).strong());
                ui.horizontal(|ui| {
                    if ui.button("Make hollow").clicked() {
                        self.hollow_selection();
                    }
                    ui.add(egui::DragValue::new(&mut self.hollow_thickness).prefix("wall ").range(1.0..=512.0));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Tie to entity:");
                    let cur = self.default_solid_class();
                    let classes = self.fgd.solid_classes().map(|c| c.name.as_str());
                    if let Some(c) = widgets::filter_combo(ui, "tiecls", &cur, None, &mut self.ent_filter, classes) {
                        self.tie_selection(&c);
                    }
                });
                ui.separator();
            }
            if let Some(e) = &primary {
                let targets: Sel = ents.iter().copied().collect();
                if ents.len() > 1 {
                    ui.label(RichText::new(format!("{} entities (editing applies to all)", ents.len())).strong());
                }
                self.entity_editor(ui, e, &targets, false, tab);
            }
        });
    }
}
