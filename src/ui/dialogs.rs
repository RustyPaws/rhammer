//! Modal-ish windows: game configurations, run map, compile log, transform, find, etc.

use crate::app::*;
#[cfg(feature = "local")]
use crate::compile::Level;
use crate::config::GameConfig;
use crate::editor::doc::{Sel, Xform};
#[cfg(feature = "local")]
use eframe::egui::Color32;
use eframe::egui::{self, RichText};
use glam::{DQuat, DVec3};

const PATH_W: f32 = 380.0;

#[derive(Clone, Copy, PartialEq, Default)]
enum CfgTab {
    #[default]
    General,
    Fgd,
    #[cfg(feature = "local")]
    Compilers,
    Defaults,
}

fn path_row(ui: &mut egui::Ui, label: &str, val: &mut String, dir: bool, ext: &[&str]) {
    #[cfg(feature = "web")]
    let _ = (dir, ext);
    ui.add(egui::Label::new(label).wrap_mode(egui::TextWrapMode::Extend));
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(val).desired_width(PATH_W));
        #[cfg(feature = "local")]
        if ui.button("…").clicked() {
            let mut d = rfd::FileDialog::new();
            if !val.is_empty() {
                let p = std::path::Path::new(val.as_str());
                if let Some(parent) = if dir { Some(p) } else { p.parent() } {
                    if parent.exists() {
                        d = d.set_directory(parent);
                    }
                }
            }
            if !ext.is_empty() {
                d = d.add_filter("file", ext);
            }
            let picked = if dir { d.pick_folder() } else { d.pick_file() };
            if let Some(p) = picked {
                *val = p.display().to_string();
            }
        }
    });
    ui.end_row();
}

/// Macros offered in the Run Map step editor.
#[cfg(feature = "local")]
const MACROS: [&str; 10] = ["$gamedir", "$path", "$file", "$ext", "$bspdir", "$bsp_exe", "$vis_exe", "$light_exe", "$game_exe", "$gameexedir"];

/// The preset / step tree with its toolbar. `sel` is the selected step of the active preset
/// (`None` = the preset itself).
#[cfg(feature = "local")]
fn run_map_tree(ui: &mut egui::Ui, cs: &mut crate::config::CompileSettings, sel: &mut Option<usize>) {
    use crate::config::{Preset, Step};
    ui.horizontal_wrapped(|ui| {
        if ui.button("+ Preset").clicked() {
            cs.presets.push(Preset::default());
            cs.active = cs.presets.len() - 1;
            *sel = None;
        }
        if ui.button("Duplicate").clicked() {
            let mut p = cs.presets[cs.active].clone();
            p.name += " copy";
            cs.presets.push(p);
            cs.active = cs.presets.len() - 1;
            *sel = None;
        }
        if ui.add_enabled(cs.presets.len() > 1, egui::Button::new("Delete preset")).clicked() {
            cs.presets.remove(cs.active);
            cs.active = cs.active.min(cs.presets.len() - 1);
            *sel = None;
        }
    });
    ui.horizontal_wrapped(|ui| {
        let steps = &mut cs.presets[cs.active].steps;
        if ui.button("+ Step").clicked() {
            steps.push(Step::default());
            *sel = Some(steps.len() - 1);
        }
        let i = sel.filter(|i| *i < steps.len());
        if ui.add_enabled(i.is_some(), egui::Button::new("Remove step")).clicked() {
            steps.remove(i.unwrap());
            *sel = None;
        }
        if ui.add_enabled(i.is_some_and(|i| i > 0), egui::Button::new("Up")).clicked() {
            let i = i.unwrap();
            steps.swap(i, i - 1);
            *sel = Some(i - 1);
        }
        if ui.add_enabled(i.is_some_and(|i| i + 1 < steps.len()), egui::Button::new("Down")).clicked() {
            let i = i.unwrap();
            steps.swap(i, i + 1);
            *sel = Some(i + 1);
        }
    });
    ui.separator();
    let active = cs.active;
    let mut new_active = active;
    let mut new_sel = *sel;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (pi, preset) in cs.presets.iter_mut().enumerate() {
            let id = ui.make_persistent_id(("rm_preset", pi));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
                .show_header(ui, |ui| {
                    if ui.selectable_label(pi == active && sel.is_none(), RichText::new(&preset.name).strong()).clicked() {
                        new_active = pi;
                        new_sel = None;
                    }
                })
                .body(|ui| {
                    for (si, step) in preset.steps.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut step.enabled, "");
                            let text = if step.enabled { RichText::new(&step.name) } else { RichText::new(&step.name).weak() };
                            if ui.selectable_label(pi == active && *sel == Some(si), text).clicked() {
                                new_active = pi;
                                new_sel = Some(si);
                            }
                        });
                    }
                });
        }
    });
    cs.active = new_active;
    *sel = new_sel;
}

/// A text field that fills the width, with a macro menu and an optional file / folder picker.
#[cfg(feature = "local")]
fn macro_field(ui: &mut egui::Ui, label: &str, text: &mut String, browse: Option<bool>) {
    ui.label(label);
    ui.horizontal(|ui| {
        let w = ui.available_width() - if browse.is_some() { 90.0 } else { 56.0 };
        ui.add(egui::TextEdit::singleline(text).desired_width(w.max(60.0)));
        ui.menu_button("$", |ui| {
            for m in MACROS {
                if ui.button(m).clicked() {
                    text.push_str(m);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("insert a variable");
        if let Some(dir) = browse {
            if ui.button("…").clicked() {
                let d = rfd::FileDialog::new();
                if let Some(p) = if dir { d.pick_folder() } else { d.pick_file() } {
                    *text = p.display().to_string();
                }
            }
        }
    });
}

/// The right side of Run Map: the active preset's name and the selected step.
#[cfg(feature = "local")]
fn run_map_editor(ui: &mut egui::Ui, cs: &mut crate::config::CompileSettings, sel: Option<usize>, game: &GameConfig, vmf: &std::path::Path) {
    use crate::config::StepKind;
    let Some(preset) = cs.presets.get_mut(cs.active) else { return };
    ui.label("Preset name");
    ui.add(egui::TextEdit::singleline(&mut preset.name).desired_width(f32::INFINITY));
    ui.separator();
    let Some(step) = sel.and_then(|i| preset.steps.get_mut(i)) else {
        ui.label(RichText::new("Select a step to edit it. Steps run top to bottom; untick one to skip it.").weak());
        ui.label(RichText::new(format!("Variables: {}", MACROS.join("  "))).small().weak());
        return;
    };
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut step.name).desired_width(200.0));
        egui::ComboBox::from_id_salt("rm_kind").selected_text(step.kind.label()).show_ui(ui, |ui| {
            for k in StepKind::ALL {
                ui.selectable_value(&mut step.kind, k, k.label());
            }
        });
        ui.checkbox(&mut step.enabled, "Enabled");
    });
    let (exe_label, args_label) = step.kind.field_labels();
    let copy = step.kind == StepKind::Copy;
    macro_field(ui, exe_label, &mut step.exe, Some(false));
    macro_field(ui, args_label, &mut step.args, copy.then_some(true));
    ui.separator();
    ui.label(RichText::new("Command line").small().weak());
    let line = if copy {
        format!("copy {}  ->  {}", game.expand(&step.exe, vmf), game.expand(&step.args, vmf))
    } else {
        format!("{} {}", game.expand(&step.exe, vmf), game.expand(&step.args, vmf))
    };
    ui.add(egui::Label::new(RichText::new(line).monospace()).wrap());
    ui.label(RichText::new(format!("Variables: {}", MACROS.join("  "))).small().weak());
}

impl App {
    pub fn dialogs(&mut self, ctx: &egui::Context) {
        self.dlg_pending(ctx);
        self.dlg_game_cfg(ctx);
        self.dlg_editor_opts(ctx);
        #[cfg(feature = "local")]
        {
            self.dlg_run_map(ctx);
            self.dlg_compile_log(ctx);
        }
        self.dlg_tex(ctx);
        self.dlg_model_viewer(ctx);
        self.dlg_sound_browser(ctx);
        self.dlg_transform(ctx);
        self.dlg_find(ctx);
        self.dlg_object_props(ctx);
        self.dlg_io_graph(ctx);
        self.dlg_map_props(ctx);
        self.dlg_about(ctx);
    }

    fn dlg_pending(&mut self, ctx: &egui::Context) {
        let Some(action) = self.win.pending_action.clone() else { return };
        let mut done: Option<bool> = None; // Some(true)=proceed
        egui::Window::new("Unsaved changes").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("The map has unsaved changes.");
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    if self.save() {
                        done = Some(true);
                    }
                }
                if ui.button("Discard").clicked() {
                    self.doc.dirty = false;
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
            });
        });
        if let Some(go) = done {
            self.win.pending_action = None;
            if go {
                self.perform(action);
            }
        }
    }

    fn dlg_editor_opts(&mut self, ctx: &egui::Context) {
        if !self.win.editor_opts {
            return;
        }
        let mut open = true;
        let mut apply = false;
        egui::Window::new("Editor options").open(&mut open).collapsible(false).default_width(520.0).show(ctx, |ui| {
            #[cfg(feature = "local")]
            {
                ui.label(RichText::new("Steam").strong());
                egui::Grid::new("editor_opts_grid").num_columns(1).show(ui, |ui| {
                    path_row(ui, "Steam folder (contains steamapps)", &mut self.steam_edit, true, &[]);
                });
                ui.label("Forces this Steam folder when resolving |appid_<id>| search paths. Leave empty to search the default Steam locations.");
                ui.separator();
                ui.checkbox(&mut self.settings.editor.restore_last_map, "Reopen the last map on startup (otherwise only after a crash)");
                ui.separator();
            }
            let ed = &mut self.settings.editor;
            ui.label(RichText::new("3D view").strong());
            ui.checkbox(&mut ed.z_freelook, "Z toggles mouse-look (locked cursor + WASD); Shift+Z maximizes. Off: Z maximizes");
            ui.horizontal(|ui| {
                ui.label("Fly speed");
                ui.add(egui::DragValue::new(&mut ed.cam_speed).range(0.25..=10.0).speed(0.05).suffix("x"));
                ui.label(RichText::new("(mouse wheel while flying)").weak());
            });
            ui.separator();
            ui.label(RichText::new("2D views").strong());
            let v = &mut ed.view2d;
            egui::Grid::new("opts_2d").num_columns(2).show(ui, |ui| {
                ui.label("Selection color");
                ui.color_edit_button_srgb(&mut v.sel_color);
                ui.end_row();
                ui.label("Selection outline width");
                ui.add(egui::DragValue::new(&mut v.sel_width).range(1.0..=6.0).speed(0.1));
                ui.end_row();
                ui.label("Background");
                ui.color_edit_button_srgb(&mut v.background);
                ui.end_row();
                ui.label("Grid brightness");
                ui.add(egui::DragValue::new(&mut v.grid_brightness).range(0.0..=3.0).speed(0.02));
                ui.end_row();
            });
            ui.checkbox(&mut v.fill_point_entities, "Fill point entity boxes");
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("OK / Apply").clicked() {
                    apply = true;
                    self.win.editor_opts = false;
                }
                if ui.button("Close").clicked() {
                    self.win.editor_opts = false;
                }
            });
        });
        if !open {
            self.win.editor_opts = false;
        }
        if apply {
            #[cfg(feature = "local")]
            {
                let dir = self.steam_edit.trim();
                self.settings.editor.steam_dir = (!dir.is_empty()).then(|| dir.to_string());
            }
            self.settings.save();
            self.reload_game_data();
            self.status = "Editor options applied".into();
        }
    }

    fn dlg_game_cfg(&mut self, ctx: &egui::Context) {
        if !self.win.game_cfg {
            return;
        }
        let mut open = true;
        let mut apply = false;
        egui::Window::new("Game Configurations").open(&mut open).default_size([720.0, 440.0]).show(ctx, |ui| {
            egui::Panel::bottom("gcfg_buttons").show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("OK / Apply").clicked() {
                        apply = true;
                        self.win.game_cfg = false;
                    }
                    if ui.button("Close").clicked() {
                        self.win.game_cfg = false;
                    }
                });
            });
            egui::Panel::left("gcfg_list").resizable(false).exact_size(200.0).show(ui, |ui| {
                ui.label(RichText::new("Configurations").strong());
                egui::ScrollArea::vertical().id_salt("gcfg_names").max_height(220.0).show(ui, |ui| {
                    for (i, g) in self.settings.games.iter().enumerate() {
                        if ui.selectable_label(self.cfg_sel == i, format!("{}{}", g.name, if self.settings.active == i { "  (active)" } else { "" })).clicked() {
                            self.cfg_sel = i;
                        }
                    }
                });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Add").clicked() {
                        self.settings.games.push(GameConfig::default());
                        self.cfg_sel = self.settings.games.len() - 1;
                    }
                    #[cfg(feature = "web")]
                    if ui
                        .add_enabled(crate::platform::web_supported(), egui::Button::new("Add game folder…"))
                        .on_hover_text("Pick a copy of a game folder (e.g. Portal 2) outside system folders; its game configuration is detected")
                        .on_disabled_hover_text("Needs a Chromium browser (Chrome, Edge)")
                        .clicked()
                    {
                        self.begin_pick();
                    }
                    if ui.add_enabled(self.settings.games.len() > 1, egui::Button::new("Remove")).clicked() {
                        #[allow(unused_variables)]
                        let removed = self.settings.games.remove(self.cfg_sel);
                        #[cfg(feature = "web")]
                        {
                            let root = removed.game_dir.split('/').next().unwrap_or("").to_string();
                            let used = self.settings.games.iter().any(|g| g.game_dir.split('/').next() == Some(root.as_str()));
                            if !root.is_empty() && !used {
                                self.web.forget(&root);
                            }
                        }
                        self.cfg_sel = self.cfg_sel.saturating_sub(1);
                        self.settings.active = self.settings.active.min(self.settings.games.len() - 1);
                    }
                    if ui.button("Set active").clicked() {
                        self.settings.active = self.cfg_sel;
                        apply = true;
                    }
                });
                ui.separator();
                #[cfg(feature = "local")]
                if ui.button("Import Hammer GameConfig.txt…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("GameConfig", &["txt"]).pick_file() {
                        match crate::config::import_hammer(&*self.vfs, &p) {
                            Ok(list) => {
                                let n = list.len();
                                self.settings.games.extend(list);
                                self.status = format!("Imported {n} configuration(s)");
                            }
                            Err(e) => self.status = format!("Import failed: {e:#}"),
                        }
                    }
                }
            });
            egui::CentralPanel::default().show(ui, |ui| {
                let sel = self.cfg_sel.min(self.settings.games.len().saturating_sub(1));
                let Some(g) = self.settings.games.get_mut(sel) else { return };
                let tab_id = egui::Id::new("gcfg_tab");
                let mut tab: CfgTab = ui.data_mut(|d| d.get_temp(tab_id).unwrap_or_default());
                ui.horizontal(|ui| {
                    let mut tabs = vec![(CfgTab::General, "General"), (CfgTab::Fgd, "Game data (FGD)")];
                    #[cfg(feature = "local")]
                    tabs.push((CfgTab::Compilers, "Compilers"));
                    tabs.push((CfgTab::Defaults, "Defaults"));
                    for (t, label) in tabs {
                        ui.selectable_value(&mut tab, t, label);
                    }
                });
                ui.data_mut(|d| d.insert_temp(tab_id, tab));
                ui.separator();
                egui::ScrollArea::vertical().id_salt("gcfg_page").auto_shrink([false, false]).show(ui, |ui| match tab {
                    CfgTab::General => {
                        egui::Grid::new("gcfg").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            ui.add(egui::Label::new("Name").wrap_mode(egui::TextWrapMode::Extend));
                            ui.add(egui::TextEdit::singleline(&mut g.name).desired_width(PATH_W));
                            ui.end_row();
                            path_row(ui, "Game directory", &mut g.game_dir, true, &[]);
                            #[cfg(feature = "local")]
                            {
                                path_row(ui, "Game executable", &mut g.game_exe, false, &["exe"]);
                                path_row(ui, "Game exe directory", &mut g.game_exe_dir, true, &[]);
                            }
                            path_row(ui, "Map directory (VMF)", &mut g.map_dir, true, &[]);
                            #[cfg(feature = "local")]
                            path_row(ui, "BSP directory (game maps)", &mut g.bsp_dir, true, &[]);
                            path_row(ui, "Prefab directory", &mut g.prefab_dir, true, &[]);
                        });
                    }
                    CfgTab::Fgd => {
                        let mut remove = None;
                        for (i, f) in g.fgds.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(f).desired_width(PATH_W + 60.0));
                                #[cfg(feature = "local")]
                                if ui.button("…").clicked() {
                                    if let Some(p) = rfd::FileDialog::new().add_filter("FGD", &["fgd"]).pick_file() {
                                        *f = p.display().to_string();
                                    }
                                }
                                if ui.button("X").on_hover_text("remove").clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                        if let Some(i) = remove {
                            g.fgds.remove(i);
                        }
                        if g.fgds.is_empty() {
                            ui.weak("No FGD files.");
                        }
                        #[cfg(feature = "local")]
                        if ui.button("Add FGD…").clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter("FGD", &["fgd"]).pick_file() {
                                g.fgds.push(p.display().to_string());
                            }
                        }
                    }
                    #[cfg(feature = "local")]
                    CfgTab::Compilers => {
                        egui::Grid::new("gcfg2").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            path_row(ui, "BSP (vbsp.exe)", &mut g.bsp_exe, false, &["exe"]);
                            path_row(ui, "VIS (vvis.exe)", &mut g.vis_exe, false, &["exe"]);
                            path_row(ui, "RAD (vrad.exe)", &mut g.light_exe, false, &["exe"]);
                        });
                    }
                    CfgTab::Defaults => {
                        egui::Grid::new("gcfg3").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            ui.add(egui::Label::new("Default texture scale").wrap_mode(egui::TextWrapMode::Extend));
                            ui.add(egui::DragValue::new(&mut g.default_texture_scale).speed(0.01).range(0.001..=16.0));
                            ui.end_row();
                            ui.add(egui::Label::new("Default lightmap scale").wrap_mode(egui::TextWrapMode::Extend));
                            ui.add(egui::DragValue::new(&mut g.default_lightmap_scale).range(1..=1024));
                            ui.end_row();
                            ui.add(egui::Label::new("Default solid entity").wrap_mode(egui::TextWrapMode::Extend));
                            ui.add(egui::TextEdit::singleline(&mut g.default_solid_entity).desired_width(PATH_W));
                            ui.end_row();
                            ui.add(egui::Label::new("Default point entity").wrap_mode(egui::TextWrapMode::Extend));
                            ui.add(egui::TextEdit::singleline(&mut g.default_point_entity).desired_width(PATH_W));
                            ui.end_row();
                            ui.add(egui::Label::new("Cordon texture").wrap_mode(egui::TextWrapMode::Extend));
                            ui.add(egui::TextEdit::singleline(&mut g.cordon_texture).desired_width(PATH_W));
                            ui.end_row();
                        });
                    }
                });
            });
        });
        if !open {
            self.win.game_cfg = false;
        }
        if apply {
            self.settings.save();
            self.reload_game_data();
            self.status = "Game configuration applied".into();
        }
    }

    #[cfg(feature = "local")]
    fn dlg_run_map(&mut self, ctx: &egui::Context) {
        if !self.win.run_map {
            return;
        }
        let mut open = true;
        let mut go = false;
        let mut cancel = false;
        let vmf = self.doc.path.clone().unwrap_or_else(|| std::path::PathBuf::from("map.vmf"));
        let game = self.game().cloned().unwrap_or_default();
        let game_name = game.name.clone();
        let mut sel = self.run_sel;
        let cs = &mut self.settings.compile;
        egui::Window::new("Run Map").open(&mut open).default_size([860.0, 480.0]).show(ctx, |ui| {
            egui::Panel::bottom("rm_bottom").show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Go!").strong()).clicked() {
                        go = true;
                    }
                    if ui.button("Close").clicked() {
                        cancel = true;
                    }
                    ui.label(RichText::new(format!("runs preset \"{}\" for {}", cs.active_preset().map_or("", |p| p.name.as_str()), game_name)).weak());
                });
            });
            egui::Panel::left("rm_tree").resizable(true).default_size(250.0).size_range(160.0..=500.0).show(ui, |ui| {
                run_map_tree(ui, cs, &mut sel);
            });
            run_map_editor(ui, cs, sel, &game, &vmf);
        });
        self.run_sel = sel;
        if !open || cancel {
            self.win.run_map = false;
            self.settings.save();
        }
        if go {
            self.run_map();
        }
    }

    #[cfg(feature = "local")]
    fn dlg_compile_log(&mut self, ctx: &egui::Context) {
        if !self.win.compile_log {
            return;
        }
        let mut open = true;
        egui::Window::new("Compile").open(&mut open).default_size([680.0, 420.0]).show(ctx, |ui| {
            if let Some(job) = &self.compile {
                ui.horizontal(|ui| {
                    if job.running {
                        ui.spinner();
                        ui.label("Running…");
                        if ui.button("Cancel").clicked() {
                            job.cancel();
                        }
                    } else if job.ok {
                        ui.colored_label(Color32::LIGHT_GREEN, "Finished");
                    } else {
                        ui.colored_label(Color32::LIGHT_RED, "Failed");
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                    for (lvl, line) in &job.log {
                        let col = match lvl {
                            Level::Info => Color32::from_gray(200),
                            Level::Step => Color32::from_rgb(120, 180, 255),
                            Level::Error => Color32::from_rgb(255, 130, 110),
                            Level::Good => Color32::LIGHT_GREEN,
                        };
                        ui.label(RichText::new(line).monospace().color(col));
                    }
                });
            } else {
                ui.label("Nothing compiled yet (F9).");
            }
        });
        if !open {
            self.win.compile_log = false;
        }
    }

    fn dlg_tex(&mut self, ctx: &egui::Context) {
        if !self.win.tex_browser {
            return;
        }
        let mut open = true;
        egui::Window::new("Texture browser").open(&mut open).default_size([560.0, 520.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Current: {}", self.cur_mat));
                if ui.button("Apply to faces").clicked() {
                    self.apply_material_to_faces();
                }
                if ui.button("Apply to selection").clicked() {
                    self.apply_material_to_selection();
                }
            });
            self.texture_grid(ui);
        });
        if !open {
            self.win.tex_browser = false;
        }
    }

    fn dlg_transform(&mut self, ctx: &egui::Context) {
        if !self.win.transform {
            return;
        }
        let mut open = true;
        let mut action: Option<Xform> = None;
        egui::Window::new("Transform selection").open(&mut open).resizable(false).show(ctx, |ui| {
            let t = &mut self.transform_dlg;
            egui::Grid::new("xform").num_columns(5).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Move");
                for v in t.mv.iter_mut() {
                    ui.add(egui::DragValue::new(v).speed(1.0));
                }
                if ui.button("Apply").clicked() {
                    action = Some(Xform::Translate(DVec3::from(t.mv)));
                }
                ui.end_row();
                ui.label("Rotate °");
                for v in t.rot.iter_mut() {
                    ui.add(egui::DragValue::new(v).speed(1.0));
                }
                if ui.button("Apply").clicked() {
                    // yaw/pitch/roll about Z/Y/X applied around the selection centre
                    let q = DQuat::from_rotation_z(t.rot[2].to_radians()) * DQuat::from_rotation_y(t.rot[1].to_radians()) * DQuat::from_rotation_x(t.rot[0].to_radians());
                    action = Some(Xform::Rotate { center: DVec3::ZERO, q });
                }
                ui.end_row();
                ui.label("Scale");
                for v in t.scale.iter_mut() {
                    ui.add(egui::DragValue::new(v).speed(0.01));
                }
                if ui.button("Apply").clicked() {
                    action = Some(Xform::Scale { origin: DVec3::ZERO, factor: DVec3::from(t.scale) });
                }
                ui.end_row();
            });
            ui.label(RichText::new("Rotate/Scale use the selection's centre as the pivot").small().weak());
        });
        if !open {
            self.win.transform = false;
        }
        if let Some(mut xf) = action {
            if let Some((a, b)) = self.doc.sel_bounds(&self.sel, &self.fgd) {
                let c = (a + b) * 0.5;
                match &mut xf {
                    Xform::Rotate { center, .. } => *center = c,
                    Xform::Scale { origin, .. } => *origin = c,
                    _ => {}
                }
                self.apply_xform(xf);
            }
        }
    }

    fn dlg_find(&mut self, ctx: &egui::Context) {
        if !self.win.find {
            return;
        }
        let mut open = true;
        let mut pick: Option<u32> = None;
        egui::Window::new("Find entities").open(&mut open).default_size([420.0, 400.0]).show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.find_text).hint_text("classname, targetname or any value…").desired_width(f32::INFINITY));
            let f = self.find_text.to_ascii_lowercase();
            let matches: Vec<(u32, String)> = self
                .doc
                .map
                .entities
                .iter()
                .filter(|e| f.is_empty() || e.props.iter().any(|(_, v)| v.to_ascii_lowercase().contains(&f)))
                .take(500)
                .map(|e| (e.id, format!("{}  {}", e.classname(), e.get("targetname").unwrap_or(""))))
                .collect();
            ui.label(RichText::new(format!("{} match(es)", matches.len())).small().weak());
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (id, label) in matches {
                    if ui.selectable_label(self.sel.contains(&id), label).clicked() {
                        pick = Some(id);
                    }
                }
            });
        });
        if !open {
            self.win.find = false;
        }
        if let Some(id) = pick {
            self.set_sel([id].into_iter().collect());
            self.frame_selection();
            self.open_properties();
        }
    }

    fn dlg_io_graph(&mut self, ctx: &egui::Context) {
        if !self.win.io_graph {
            return;
        }
        let mut open = true;
        let mut pick: Option<u32> = None;
        egui::Window::new("Entity I/O graph").open(&mut open).default_size([900.0, 600.0]).show(ctx, |ui| {
            pick = self.io_graph.show(ui, &self.doc.map, self.doc.version, &self.sel, self.sel_stamp);
        });
        if !open {
            self.win.io_graph = false;
        }
        if let Some(id) = pick {
            self.set_sel([id].into_iter().collect());
            self.frame_selection();
            self.open_properties();
        }
    }

    fn dlg_object_props(&mut self, ctx: &egui::Context) {
        if !self.win.object_props {
            return;
        }
        // fades while something is dragged in a view, so the view under it stays visible
        // (the view holds the pointer for the whole drag, so the window can't steal it)
        let t = ctx.animate_bool_with_time(egui::Id::new("object_props_fade"), self.view_busy, 0.15);
        let alpha = egui::lerp(1.0..=0.25, t);
        let style = ctx.global_style();
        let title = RichText::new("Object Properties").color(style.visuals.text_color().gamma_multiply(alpha));
        let mut open = true;
        egui::Window::new(title)
            .id(egui::Id::new("object_props"))
            .frame(egui::Frame::window(&style).multiply_with_opacity(alpha))
            .open(&mut open)
            .default_size([380.0, 540.0])
            .show(ctx, |ui| {
                ui.multiply_opacity(alpha);
                self.object_props_ui(ui);
            });
        if !open {
            self.win.object_props = false;
        }
    }

    fn dlg_map_props(&mut self, ctx: &egui::Context) {
        if !self.win.map_props {
            return;
        }
        let mut open = true;
        egui::Window::new("Map properties (worldspawn)").open(&mut open).default_size([420.0, 480.0]).show(ctx, |ui| {
            let w = self.doc.map.world.clone();
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.entity_editor(ui, &w, &Sel::new(), true, crate::ui::props::ObjTab::Properties);
            });
        });
        if !open {
            self.win.map_props = false;
        }
    }

    fn dlg_about(&mut self, ctx: &egui::Context) {
        if !self.win.about {
            return;
        }
        let mut open = true;
        egui::Window::new("About").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.heading("rhammer");
            ui.label("A Hammer-compatible Source level editor written in Rust + egui.");
            ui.label("Reads and writes VMF; unknown blocks are preserved verbatim.");
            ui.separator();
            ui.label("Views: RMB look + WASD/QE fly in 3D, MMB/RMB pan and wheel zoom in 2D, Z maximizes a view.");
            let tools: Vec<String> = crate::app::Tool::ALL.iter().map(|t| format!("Shift+{} {}", t.shortcut().name(), t.name().to_lowercase())).collect();
            ui.label(format!("Tools: {}.", tools.join(", ")));
            ui.label("Enter creates the block/applies the clip. Ctrl+H hollow, Ctrl+T tie to entity, F9 run map.");
        });
        if !open {
            self.win.about = false;
        }
    }
}
