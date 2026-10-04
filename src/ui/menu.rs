//! The main menu bar.

use crate::app::*;
use crate::editor::doc::Sel;
use crate::ui::layout::Pane;
use eframe::egui;
use glam::DVec3;

impl App {
    pub(crate) fn menu(&mut self, ui: &mut egui::Ui) {
        let mut bar: Vec<egui::Response> = Vec::new();
        egui::MenuBar::new().ui(ui, |ui| {
            bar.push(ui.menu_button("File", |ui| {
                if ui.button("New          Ctrl+N").clicked() {
                    self.request(PendingAction::New);
                    ui.close();
                }
                let io_ok = Self::file_io_ok();
                const NEEDS_CHROMIUM: &str = "Needs a Chromium browser (Chrome, Edge)";
                if ui.add_enabled(io_ok, egui::Button::new("Open...        Ctrl+O")).on_disabled_hover_text(NEEDS_CHROMIUM).clicked() {
                    self.request(PendingAction::Open(None));
                    ui.close();
                }
                #[cfg(feature = "web")]
                {
                    ui.separator();
                    let ok = crate::platform::web_supported();
                    if ui.add_enabled(ok, egui::Button::new("Open game folder...")).on_disabled_hover_text("Needs a Chromium browser (Chrome, Edge)").clicked() {
                        self.begin_pick();
                        ui.close();
                    }
                    ui.separator();
                }
                if ui.add_enabled(io_ok, egui::Button::new("Save         Ctrl+S")).on_disabled_hover_text(NEEDS_CHROMIUM).clicked() {
                    self.save();
                    ui.close();
                }
                if ui.add_enabled(io_ok, egui::Button::new("Save As...     Ctrl+Shift+S")).on_disabled_hover_text(NEEDS_CHROMIUM).clicked() {
                    self.save_as();
                    ui.close();
                }
                #[cfg(feature = "local")]
                {
                    ui.separator();
                    if ui.button("Run Map...     F9").clicked() {
                        self.win.run_map = true;
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    self.request(PendingAction::Quit);
                    ui.close();
                }
            }).response);
            bar.push(ui.menu_button("Edit", |ui| {
                if ui.add_enabled(self.doc.can_undo(), egui::Button::new("Undo   Ctrl+Z")).clicked() {
                    self.undo();
                    ui.close();
                }
                if ui.add_enabled(self.doc.can_redo(), egui::Button::new("Redo   Ctrl+Y")).clicked() {
                    self.redo();
                    ui.close();
                }
                ui.separator();
                if ui.button("Cut    Ctrl+X").clicked() {
                    self.cut_selection();
                    ui.close();
                }
                if ui.button("Copy   Ctrl+C").clicked() {
                    self.copy_selection();
                    ui.close();
                }
                if ui.button("Paste  Ctrl+V").clicked() {
                    self.paste_clipboard();
                    ui.close();
                }
                if ui.button("Duplicate  Ctrl+D").clicked() {
                    self.duplicate_selection();
                    ui.close();
                }
                if ui.button("Delete  Del").clicked() {
                    self.delete_selection();
                    ui.close();
                }
                ui.separator();
                if ui.button("Select all  Ctrl+A").clicked() {
                    let all: Sel = self.doc.all_ids().into_iter().filter(|i| !self.doc.is_hidden(*i)).collect();
                    self.set_sel(all);
                    ui.close();
                }
                if ui.button("Find...  Ctrl+F").clicked() {
                    self.win.find = true;
                    ui.close();
                }
                if ui.button("Properties...  Alt+Enter").clicked() {
                    self.open_properties();
                    ui.close();
                }
                if ui.button("Map properties...").clicked() {
                    self.win.map_props = true;
                    ui.close();
                }
            }).response);
            bar.push(ui.menu_button("Tools", |ui| {
                if ui.button("Transform...   Ctrl+M").clicked() {
                    self.win.transform = true;
                    ui.close();
                }
                if ui.button("Make hollow  Ctrl+H").clicked() {
                    self.hollow_selection();
                    ui.close();
                }
                if ui.button("Tie to entity  Ctrl+T").clicked() {
                    let c = self.default_solid_class();
                    self.tie_selection(&c);
                    ui.close();
                }
                if ui.button("Move to world").clicked() {
                    self.move_to_world();
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Rotate", |ui| {
                    for (label, axis, deg) in [
                        ("Z +90 deg", DVec3::Z, 90.0),
                        ("Z -90 deg", DVec3::Z, -90.0),
                        ("X +90 deg", DVec3::X, 90.0),
                        ("X -90 deg", DVec3::X, -90.0),
                        ("Y +90 deg", DVec3::Y, 90.0),
                        ("Y -90 deg", DVec3::Y, -90.0),
                    ] {
                        if ui.button(label).clicked() {
                            self.rotate_selection(axis, deg);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Mirror", |ui| {
                    for (label, ax) in [("X axis", 0), ("Y axis", 1), ("Z axis", 2)] {
                        if ui.button(label).clicked() {
                            self.mirror_selection(ax);
                            ui.close();
                        }
                    }
                });
                ui.separator();
                ui.add(egui::DragValue::new(&mut self.hollow_thickness).prefix("Hollow wall: ").range(1.0..=512.0));
            }).response);
            bar.push(ui.menu_button("View", |ui| {
                ui.checkbox(&mut self.show_grid, "Show grid  (G)");
                ui.checkbox(&mut self.wireframe, "3D wireframe  (F5)");
                ui.checkbox(&mut self.show_entity_names, "Entity names");
                ui.checkbox(&mut self.anim_play, "Animate models");
                if ui.button("Frame selection  Shift+F").clicked() {
                    self.frame_selection();
                    ui.close();
                }
                if ui.button("Frame all").clicked() {
                    self.frame_all();
                    ui.close();
                }
                ui.separator();
                for (i, n) in VIEW_NAMES.iter().enumerate() {
                    if ui.selectable_label(self.maximized == Some(i), format!("Maximize {n}")).clicked() {
                        self.maximized = if self.maximized == Some(i) { None } else { Some(i) };
                        ui.close();
                    }
                }
                if ui.button("4 views").clicked() {
                    self.maximized = None;
                    ui.close();
                }
                ui.separator();
                for p in Pane::PANELS {
                    let shown = self.settings.ui.dock.find_tab(&p).is_some();
                    if ui.selectable_label(shown, p.name()).clicked() {
                        self.settings.ui.toggle(p);
                        ui.close();
                    }
                }
                if ui.button("Reset layout").clicked() {
                    self.reset_layout();
                    ui.close();
                }
            }).response);
            bar.push(ui.menu_button("Map", |ui| {
                #[cfg(feature = "local")]
                {
                    if ui.button("Run map...  F9").clicked() {
                        self.win.run_map = true;
                        ui.close();
                    }
                    if ui.button("Compile log").clicked() {
                        self.win.compile_log = true;
                        ui.close();
                    }
                }
                if ui.button("Texture browser...").clicked() {
                    self.win.tex_browser = true;
                    ui.close();
                }
                if ui.button("Model viewer...").clicked() {
                    self.win.model_viewer = true;
                    ui.close();
                }
                if ui.button("Entity I/O graph...").clicked() {
                    self.win.io_graph = true;
                    ui.close();
                }
            }).response);
            bar.push(ui.menu_button("Options", |ui| {
                if ui.button("Game configurations...").clicked() {
                    self.win.game_cfg = true;
                    self.cfg_sel = self.settings.active;
                    ui.close();
                }
                #[cfg(feature = "local")]
                if ui.button("Editor options...").clicked() {
                    self.win.editor_opts = true;
                    self.steam_edit = self.settings.editor.steam_dir.clone().unwrap_or_default();
                    ui.close();
                }
            }).response);
            bar.push(ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    self.win.about = true;
                    ui.close();
                }
            }).response);
        });
        switch_on_hover(ui.ctx(), &bar);
    }
}

/// While one of the bar's menus is open, hovering another bar button opens that one instead,
/// like in desktop menu bars. Other popups (combo boxes, context menus) are left alone.
fn switch_on_hover(ctx: &egui::Context, bar: &[egui::Response]) {
    let id = |r: &egui::Response| egui::Popup::default_response_id(r);
    if !bar.iter().any(|r| egui::Popup::is_id_open(ctx, id(r))) {
        return;
    }
    if let Some(r) = bar.iter().find(|r| r.hovered() && !egui::Popup::is_id_open(ctx, id(r))) {
        egui::Popup::open_id(ctx, id(r));
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs one frame of a two-menu bar; returns (File rect, Edit rect, which popups are open).
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> (egui::Rect, egui::Rect, [bool; 2]) {
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), events, ..Default::default() };
        let mut out = None;
        let mut full = ctx.run_ui(input, |ui| {
            let mut bar = Vec::new();
            egui::MenuBar::new().ui(ui, |ui| {
                bar.push(ui.menu_button("File", |ui| ui.label("f")).response);
                bar.push(ui.menu_button("Edit", |ui| ui.label("e")).response);
            });
            switch_on_hover(ui.ctx(), &bar);
            let open = |i: usize| egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&bar[i]));
            out = Some((bar[0].rect, bar[1].rect, [open(0), open(1)]));
        });
        full.textures_delta.clear();
        out.unwrap()
    }

    #[test]
    fn hovering_another_bar_menu_switches_to_it() {
        let ctx = egui::Context::default();
        let (file, edit, _) = frame(&ctx, vec![]);
        let click = |p: egui::Pos2, pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&ctx, vec![egui::Event::PointerMoved(file.center())]);
        frame(&ctx, vec![click(file.center(), true)]);
        let (_, _, open) = frame(&ctx, vec![click(file.center(), false)]);
        assert_eq!(open, [true, false], "File opens on click");
        frame(&ctx, vec![egui::Event::PointerMoved(edit.center())]);
        let (_, _, open) = frame(&ctx, vec![]);
        assert_eq!(open, [false, true], "hover switches to Edit");
    }

    #[test]
    fn hovering_does_nothing_when_no_menu_is_open() {
        let ctx = egui::Context::default();
        let (_, edit, _) = frame(&ctx, vec![]);
        frame(&ctx, vec![egui::Event::PointerMoved(edit.center())]);
        let (_, _, open) = frame(&ctx, vec![]);
        assert_eq!(open, [false, false]);
    }
}
