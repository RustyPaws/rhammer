//! The Options pane (grid / snap and the active tool's settings) and the status bar.

use crate::app::{App, Tool};
use crate::editor::geom::Primitive;
#[cfg(feature = "local")]
use eframe::egui::Color32;
use eframe::egui::{self, RichText};

impl App {
    /// Grid / snap toggles and the settings of the active tool.
    pub fn options_pane(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
            {
                ui.label("Grid:");
                egui::ComboBox::from_id_salt("grid").selected_text(format!("{}", self.grid)).width(ui.available_width().clamp(30.0, 60.0)).show_ui(ui, |ui| {
                    for g in [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0] {
                        ui.selectable_value(&mut self.grid, g, format!("{g}"));
                    }
                });
            }
            ui.checkbox(&mut self.snap, "Snap");
            ui.checkbox(&mut self.show_grid, "Grid");
            ui.checkbox(&mut self.tex_lock, "Texture lock");
            ui.end_row();
            match self.tool {
                Tool::Block => {
                    egui::ComboBox::from_id_salt("prim").selected_text(self.primitive.name()).show_ui(ui, |ui| {
                        for p in Primitive::ALL {
                            ui.selectable_value(&mut self.primitive, p, p.name());
                        }
                    });
                    if matches!(self.primitive, Primitive::Cylinder | Primitive::Cone) {
                        ui.add(egui::DragValue::new(&mut self.prim_sides).range(3..=64).prefix("sides "));
                    }
                    ui.add(egui::Label::new(format!("Material: {}", self.cur_mat)).wrap());
                    if ui.button("...").clicked() {
                        self.win.tex_browser = true;
                    }
                    if ui.add_enabled(self.block.is_some(), egui::Button::new("Create (Enter)")).clicked() {
                        self.commit_block();
                    }
                }
                Tool::Entity => {
                    ui.label("Class:");
                    self.entity_class_picker(ui);
                    ui.label(RichText::new("click in a view to place").weak());
                }
                Tool::Clip => {
                    for (i, n) in ["Keep both", "Keep front", "Keep back"].iter().enumerate() {
                        ui.radio_value(&mut self.clip.mode, i as u8, *n);
                    }
                    if ui.add_enabled(self.clip.p0.is_some() && self.clip.p1.is_some(), egui::Button::new("Apply (Enter)")).clicked() {
                        self.commit_clip();
                    }
                }
                Tool::Texture => {
                    ui.add(egui::Label::new(format!("Material: {}", self.cur_mat)).wrap());
                    ui.label(RichText::new("LMB select face - RMB apply texture").weak());
                }
                Tool::Vertex => {
                    ui.label(RichText::new("select a brush - drag corners or edge dots - Ctrl+F merges selected corners").weak());
                }
                Tool::Select => {
                    ui.label(RichText::new("click / drag to select - Shift+drag clones").weak());
                }
            }
            });
        });
    }

    pub fn entity_class_picker(&mut self, ui: &mut egui::Ui) {
        let w = (ui.available_width() - 8.0).clamp(60.0, 220.0);
        let classes = self.fgd.point_classes().map(|c| c.name.as_str());
        if let Some(n) = crate::ui::props::widgets::filter_combo(ui, "entclass", &self.ent_class, Some(w), &mut self.ent_filter, classes) {
            self.ent_class = n;
        }
    }

    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.status);
            ui.separator();
            if let Some(p) = self.hover_world {
                ui.monospace(format!("{:.0} {:.0} {:.0}", p.x, p.y, p.z));
                ui.separator();
            }
            ui.label(format!("{} selected", self.sel.len()));
            if let Some((a, b)) = self.doc.sel_bounds(&self.sel, &self.fgd) {
                let s = b - a;
                ui.separator();
                ui.monospace(format!("size {:.0} x {:.0} x {:.0}", s.x, s.y, s.z));
            }
            ui.separator();
            ui.label(format!("{} brushes, {} entities", self.doc.map.world.solids.len(), self.doc.map.entities.len()));
            #[cfg(feature = "local")]
            if let Some(job) = &self.compile {
                ui.separator();
                if job.running {
                    ui.spinner();
                    ui.label("compiling...");
                } else if job.ok {
                    ui.colored_label(Color32::LIGHT_GREEN, "compile ok");
                } else {
                    ui.colored_label(Color32::LIGHT_RED, "compile failed");
                }
            }
        });
    }
}
