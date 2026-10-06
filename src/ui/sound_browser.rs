//! Sound browser: searchable list of the game's sound script entries and sound files with
//! preview playback. Doubles as the picker for `sound` keys.

use crate::app::App;
use crate::assets::sounds::{self, SoundLists};
use crate::editor::doc::Sel;
use crate::ui::model_viewer::PickTarget;
use eframe::egui::{self, RichText};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Scripts,
    Files,
}

#[derive(Default)]
pub struct SoundBrowser {
    lists: Option<SoundLists>,
    tab: Tab,
    filter: String,
    pub selected: String,
    /// Set when opened from an entity's sound key: the window becomes a picker.
    pub pick: Option<PickTarget>,
    /// Indices into the current tab's list that match the filter, with the key they were built for.
    shown: Vec<usize>,
    shown_key: Option<(String, bool, usize)>,
    #[cfg(feature = "local")]
    player: crate::assets::audio::Player,
    message: String,
}

impl SoundBrowser {
    fn names(&self) -> Vec<&str> {
        match (&self.lists, self.tab) {
            (Some(l), Tab::Scripts) => l.scripts.iter().map(|s| s.0.as_str()).collect(),
            (Some(l), Tab::Files) => l.files.iter().map(String::as_str).collect(),
            _ => vec![],
        }
    }

    /// The sound file behind the selection, relative to `sound/`.
    fn wave(&self) -> Option<String> {
        let l = self.lists.as_ref()?;
        match self.tab {
            Tab::Files => Some(self.selected.clone()),
            Tab::Scripts => l.scripts.iter().find(|s| s.0.eq_ignore_ascii_case(&self.selected)).map(|s| s.1.clone()).filter(|w| !w.is_empty()),
        }
    }
}

impl App {
    /// Open the sound browser as a picker for `key` on `ids`, starting at `current`.
    pub fn pick_sound(&mut self, ids: Sel, key: &str, current: &str) {
        let sb = &mut self.snd;
        sb.pick = Some(PickTarget { ids, key: key.to_string() });
        sb.selected = current.to_string();
        sb.tab = if current.contains('/') || current.contains('\\') || current.to_ascii_lowercase().ends_with(".wav") || current.to_ascii_lowercase().ends_with(".mp3") { Tab::Files } else { Tab::Scripts };
        sb.filter.clear();
        self.win.sound_browser = true;
    }

    pub fn dlg_sound_browser(&mut self, ctx: &egui::Context) {
        if !self.win.sound_browser {
            return;
        }
        let mut sb = std::mem::take(&mut self.snd);
        if sb.lists.is_none() {
            sb.lists = Some(sounds::load(&self.mats.fs));
        }
        let (mut open, mut accept, mut cancel) = (true, false, false);
        let picking = sb.pick.is_some();
        egui::Window::new(if picking { "Select sound" } else { "Sound browser" }).id(egui::Id::new("sound_browser")).open(&mut open).default_size([520.0, 560.0]).show(ctx, |ui| {
            egui::Panel::bottom("sb_bottom").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(if sb.selected.is_empty() { "(nothing selected)" } else { sb.selected.as_str() }).monospace()).truncate());
                });
                ui.horizontal(|ui| {
                    #[cfg(feature = "local")]
                    {
                        let playing = sb.player.is_playing();
                        if ui.add_enabled(!sb.selected.is_empty(), egui::Button::new(if playing { "Stop" } else { "Play" })).clicked() {
                            if playing {
                                sb.player.stop();
                            } else if let Some(w) = sb.wave() {
                                sb.message.clear();
                                match self.mats.fs.read(&format!("sound/{w}")) {
                                    Some(bytes) => {
                                        if let Err(e) = sb.player.play(bytes) {
                                            sb.message = e;
                                        }
                                    }
                                    None => sb.message = format!("sound/{w} not found"),
                                }
                            } else {
                                sb.message = "this entry has no wave".into();
                            }
                        }
                        if playing {
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
                        }
                    }
                    #[cfg(feature = "web")]
                    ui.label(RichText::new("preview needs the desktop build").weak());
                    if !sb.message.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(255, 130, 110), &sb.message);
                    }
                    if picking {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                            if ui.add_enabled(!sb.selected.is_empty(), egui::Button::new(RichText::new("OK").strong())).clicked() {
                                accept = true;
                            }
                        });
                    }
                });
            });
            ui.horizontal(|ui| {
                let (ns, nf) = sb.lists.as_ref().map_or((0, 0), |l| (l.scripts.len(), l.files.len()));
                ui.selectable_value(&mut sb.tab, Tab::Scripts, format!("Sound scripts ({ns})"));
                ui.selectable_value(&mut sb.tab, Tab::Files, format!("Files ({nf})"));
            });
            ui.add(egui::TextEdit::singleline(&mut sb.filter).hint_text("search…").desired_width(f32::INFINITY));
            let key = (sb.filter.clone(), sb.tab == Tab::Files, sb.lists.as_ref().map_or(0, |l| l.files.len() + l.scripts.len()));
            if sb.shown_key.as_ref() != Some(&key) {
                let f = sb.filter.to_ascii_lowercase();
                sb.shown = sb.names().iter().enumerate().filter(|(_, n)| f.is_empty() || n.to_ascii_lowercase().contains(&f)).map(|(i, _)| i).collect();
                sb.shown_key = Some(key);
            }
            ui.separator();
            let row_h = ui.spacing().interact_size.y;
            let total = sb.shown.len();
            let mut clicked: Option<(String, bool)> = None;
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, total, |ui, range| {
                let names = sb.names();
                for &i in &sb.shown[range] {
                    let n = names[i];
                    let r = ui.selectable_label(n.eq_ignore_ascii_case(&sb.selected), n);
                    if r.clicked() || r.double_clicked() {
                        clicked = Some((n.to_string(), r.double_clicked()));
                    }
                }
            });
            if let Some((n, double)) = clicked {
                sb.selected = n;
                sb.message.clear();
                if double && picking {
                    accept = true;
                }
            }
        });
        if accept {
            if let Some(t) = sb.pick.take() {
                self.doc.checkpoint();
                self.doc.set_prop(&t.ids, &t.key, &sb.selected);
                self.doc.touch();
                self.status = format!("{} = {}", t.key, sb.selected);
            }
            open = false;
        }
        if !open || cancel {
            self.win.sound_browser = false;
            sb.pick = None;
            #[cfg(feature = "local")]
            sb.player.stop();
        }
        self.snd = sb;
    }
}
