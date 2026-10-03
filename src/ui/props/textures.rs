//! Texture tab and face editing.

use crate::editor::geom;
use crate::formats::vmf::{self};
use eframe::egui::{self, Color32, RichText};
use super::*;

impl App {
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

    pub(crate) fn for_each_face(&mut self, mut f: impl FnMut(&mut crate::formats::vmf::Side)) {
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

    pub(crate) fn texture_tab(&mut self, ui: &mut egui::Ui) {
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
}
