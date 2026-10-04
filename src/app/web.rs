//! Browser only: folder picking, async file events and the notices around them.

use super::App;
use crate::platform::WebEvent;
use eframe::egui::{self, Align2};
use std::path::{Path, PathBuf};

impl App {
    /// Browser only: starts picking a game folder, after a notice about system folders (unless
    /// the user asked not to see it again). Call from a click handler.
    #[cfg(feature = "web")]
    pub fn begin_pick(&mut self) {
        if crate::platform::storage_get("rhammer_skip_pick_info").as_deref() == Some("1") {
            self.web.start_pick();
        } else {
            self.pick_info = true;
        }
    }

    /// Browser only: finishes a folder pick and keeps loading game data until it is complete.
    #[cfg(feature = "web")]
    pub(crate) fn web_poll(&mut self, ctx: &egui::Context) {
        for ev in self.web.take_events() {
            match ev {
                WebEvent::Picked(Ok((root, list))) => {
                    let mut first = None;
                    for g in list {
                        let at = match self.settings.games.iter().position(|x| x.name == g.name && x.game_dir == g.game_dir) {
                            Some(i) => {
                                self.settings.games[i] = g;
                                i
                            }
                            None => {
                                self.settings.games.push(g);
                                self.settings.games.len() - 1
                            }
                        };
                        first.get_or_insert(at);
                    }
                    self.settings.active = first.unwrap_or(0);
                    self.cfg_sel = self.settings.active;
                    self.settings.save();
                    self.status = format!("Opened game folder '{root}'");
                    self.reload_game_data();
                }
                WebEvent::Picked(Err(e)) => self.status = e,
                WebEvent::Restored { locked } => {
                    self.locked_roots = locked;
                    if self.settings.active_game().is_some() {
                        self.reload_game_data();
                    }
                }
                WebEvent::Opened(Ok((path, bytes))) => self.open_map_bytes(&bytes, Path::new(&path)),
                WebEvent::Opened(Err(e)) => self.status = if e == "Cancelled" { e } else { format!("Open failed: {e}") },
                WebEvent::Saved { path, result: Ok(()) } => {
                    if !path.is_empty() {
                        self.doc.path = Some(PathBuf::from(&path));
                        self.settings.last_map = path.clone();
                        self.settings.save();
                    }
                    self.doc.dirty = false;
                    self.status = format!("Saved {path}");
                }
                WebEvent::Saved { result: Err(e), .. } => self.status = if e == "Cancelled" { "Save cancelled".into() } else { format!("Save failed: {e}") },
            }
        }
        if self.pick_info {
            let mut close = false;
            egui::Window::new("Choose your game folder").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.set_max_width(440.0);
                ui.label("Pick the folder of the game itself, for example 'Portal 2' (the one that contains 'bin' and 'portal2').");
                ui.add_space(4.0);
                ui.colored_label(egui::Color32::from_rgb(255, 190, 90), "Browsers refuse system folders.");
                ui.label(
                    r"Steam usually lives in C:\Program Files (x86)\Steam, which the browser will not let a website open. Copy the game folder (steamapps/common/Portal 2) to your own folder first, for example Documents or Desktop, and pick the copy. The copy is only read and written by this editor; maps you save go into it.",
                );
                ui.add_space(4.0);
                ui.checkbox(&mut self.pick_info_skip, "Do not show this again");
                ui.horizontal(|ui| {
                    if ui.button("Choose folder...").clicked() {
                        if self.pick_info_skip {
                            crate::platform::storage_set("rhammer_skip_pick_info", "1");
                        }
                        self.web.start_pick();
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
            if close {
                self.pick_info = false;
            }
        }
        if !self.unsupported_ack && !crate::platform::web_supported() {
            egui::Window::new("Browser not supported").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.colored_label(egui::Color32::from_rgb(255, 190, 90), "This browser does not support the File System Access API.");
                ui.label(
                    "rhammer needs it to read your game folder (textures, models, entity definitions) and to open and save .vmf files. Use a Chromium-based browser such as Google Chrome, Microsoft Edge or Opera on a desktop computer. Firefox and Safari do not provide this API.",
                );
                ui.label("You can continue, but game files cannot be loaded and maps cannot be opened or saved.");
                if ui.button("Continue anyway").clicked() {
                    self.unsupported_ack = true;
                }
            });
        }
        if !self.locked_roots.is_empty() {
            egui::Window::new("Game folders").collapsible(false).resizable(false).anchor(Align2::CENTER_TOP, [0.0, 40.0]).show(ctx, |ui| {
                ui.label(format!("The browser needs your permission to use: {}", self.locked_roots.join(", ")));
                if ui.button("Reconnect").clicked() {
                    self.web.reconnect();
                }
            });
        }
        if self.loading && self.vfs.pending() == 0 {
            self.reload_game_data();
        }
        if self.loading {
            egui::Window::new("Loading game data").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Reading game files ({} requests in flight)", self.vfs.pending()));
                });
            });
        }
    }
}
