//! Global keyboard shortcuts.

use super::{App, PendingAction, Tool};
use crate::editor::doc::Sel;
use eframe::egui::{self, Key};

impl App {
    pub(crate) fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.freelook || ctx.egui_wants_keyboard_input() || ctx.input(|i| i.pointer.secondary_down()) {
            return; // typing, or flying the 3D camera (RMB held / freelook)
        }
        let (ctrl, shift, alt) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift, i.modifiers.alt));
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        // egui turns Ctrl+C/X/V into Copy/Cut/Paste events instead of key presses
        let (ev_copy, ev_cut, ev_paste) = ctx.input(|i| {
            let has = |f: fn(&egui::Event) -> bool| i.events.iter().any(f);
            (
                has(|e| matches!(e, egui::Event::Copy)),
                has(|e| matches!(e, egui::Event::Cut)),
                has(|e| matches!(e, egui::Event::Paste(_))),
            )
        });
        if ev_copy {
            self.copy_selection();
        }
        if ev_cut {
            self.cut_selection();
        }
        if ev_paste {
            self.paste_clipboard();
        }
        if ctrl {
            if pressed(Key::Z) {
                if shift { self.redo() } else { self.undo() }
            }
            if pressed(Key::Y) {
                self.redo();
            }
            if pressed(Key::S) && Self::file_io_ok() {
                if shift { self.save_as(); } else { self.save(); }
            }
            if pressed(Key::O) && Self::file_io_ok() {
                self.request(PendingAction::Open(None));
            }
            if pressed(Key::N) {
                self.request(PendingAction::New);
            }
            if pressed(Key::Q) {
                self.request(PendingAction::Quit);
            }
            if pressed(Key::D) {
                self.duplicate_selection();
            }
            if pressed(Key::H) {
                self.hollow_selection();
            }
            if pressed(Key::M) {
                self.win.transform = true;
            }
            if pressed(Key::F) && !(self.tool == Tool::Vertex && self.vertex_merge()) {
                self.win.find = true;
            }
            if pressed(Key::A) {
                let all: Sel = self.doc.all_ids().into_iter().filter(|i| !self.doc.is_hidden(*i)).collect();
                self.set_sel(all);
            }
            if pressed(Key::T) {
                let c = self.default_solid_class();
                self.tie_selection(&c);
            }
            return;
        }
        if shift {
            if let Some(t) = Tool::ALL.into_iter().find(|t| pressed(t.shortcut())) {
                self.set_tool(t);
            }
            if pressed(Key::F) { self.frame_selection(); }
        }
        if !shift && !alt {
            if pressed(Key::Delete) || pressed(Key::Backspace) {
                self.delete_selection();
            }
            if pressed(Key::Escape) {
                self.block = None;
                self.clip.p0 = None;
                self.clip.p1 = None;
                self.drag = None;
                self.vtx.sel.clear();
                if !self.sel.is_empty() {
                    self.set_sel(Sel::new());
                }
                self.faces.clear();
                self.bump_sel();
            }
            if pressed(Key::Enter) && alt {
                self.open_properties();
            } else if pressed(Key::Enter) {
                match self.tool {
                    Tool::Block => self.commit_block(),
                    Tool::Clip => self.commit_clip(),
                    _ => {}
                }
            }
            #[cfg(feature = "local")]
            if pressed(Key::F9) {
                self.win.run_map = true;
            }
            if pressed(Key::G) {
                self.show_grid = !self.show_grid;
            }
            if pressed(Key::OpenBracket) {
                self.grid = (self.grid / 2.0).max(1.0);
            }
            if pressed(Key::CloseBracket) {
                self.grid = (self.grid * 2.0).min(1024.0);
            }
        }
        #[cfg(feature = "local")]
        if pressed(Key::F9) && shift {
            self.win.run_map = true;
        }
        if pressed(Key::F5) {
            self.wireframe = !self.wireframe;
        }
    }
}
