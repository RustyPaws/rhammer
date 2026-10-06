//! New / open / save, the "unsaved changes" flow and Run Map.

use super::{App, PendingAction};
use crate::editor::doc::{Doc, Sel};
use crate::formats::vmf::Map;
use eframe::egui;
use std::path::Path;

/// Marker file that exists while the editor runs; finding it at startup means the last run crashed.
#[cfg(feature = "local")]
pub(super) const SESSION_MARKER: &str = "rhammer.session";

#[cfg(feature = "local")]
pub(crate) fn clear_session_marker() {
    let _ = std::fs::remove_file(SESSION_MARKER);
}

impl App {
    pub fn new_map(&mut self) {
        self.doc = Doc::new(Map::new_empty(), None);
        self.anim_preview.clear();
        self.reset_derived();
        self.set_sel(Sel::new());
        self.faces.clear();
        self.block = None;
        self.status = "New map".into();
    }

    #[cfg(feature = "local")]
    pub fn open_path(&mut self, p: &Path) {
        match Map::load(p) {
            Ok(m) => self.open_map(m, p),
            Err(e) => self.status = format!("Open failed: {e:#}"),
        }
    }

    /// Opens a map that is already in the game files (browser build).
    #[cfg(feature = "web")]
    pub fn open_path(&mut self, p: &Path) {
        match self.vfs.read(p) {
            Some(b) => self.open_map_bytes(&b, p),
            None => self.status = format!("Open failed: {} is not available", p.display()),
        }
    }

    #[cfg(feature = "web")]
    pub(crate) fn open_map_bytes(&mut self, bytes: &[u8], p: &Path) {
        match Map::parse(&String::from_utf8_lossy(bytes)) {
            Ok(m) => self.open_map(m, p),
            Err(e) => self.status = format!("Open failed: {e:#}"),
        }
    }

    fn open_map(&mut self, m: Map, p: &Path) {
        self.doc = Doc::new(m, Some(p.to_path_buf()));
        self.anim_preview.clear();
        self.reset_derived();
        self.set_sel(Sel::new());
        self.faces.clear();
        self.block = None;
        self.grid = self.doc.map.grid_spacing().max(1.0);
        self.settings.last_map = p.display().to_string();
        self.settings.save();
        self.frame_all();
        self.status = format!("Opened {}", p.display());
    }

    #[cfg(feature = "web")]
    pub fn open_dialog(&mut self) {
        let start = self.game().map(|g| g.map_dir.clone()).unwrap_or_default();
        self.web.start_open(&start);
    }

    #[cfg(feature = "local")]
    pub fn open_dialog(&mut self) {
        let mut d = rfd::FileDialog::new().add_filter("Valve Map", &["vmf"]).add_filter("All files", &["*"]);
        if let Some(g) = self.game() {
            if !g.map_dir.is_empty() {
                d = d.set_directory(&g.map_dir);
            }
        }
        if let Some(p) = d.pick_file() {
            self.open_path(&p);
        }
    }

    /// Browser builds need the File System Access API (Chromium) to open or save maps.
    pub fn file_io_ok() -> bool {
        #[cfg(feature = "web")]
        {
            crate::platform::web_supported()
        }
        #[cfg(not(feature = "web"))]
        {
            true
        }
    }

    pub fn save(&mut self) -> bool {
        if !Self::file_io_ok() {
            return false;
        }
        match self.doc.path.clone() {
            Some(p) => self.save_to(&p),
            None => self.save_as(),
        }
    }

    #[cfg(feature = "web")]
    pub fn save_as(&mut self) -> bool {
        self.prepare_save();
        let text = self.doc.map.to_text();
        let suggested = self.doc.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "untitled.vmf".into());
        let start = self.game().map(|g| g.map_dir.clone()).unwrap_or_default();
        self.web.start_save_as(&start, &suggested, text.into_bytes());
        self.status = "Choose where to save...".into();
        // the file is written once the browser dialog closes, so this cannot report success yet
        false
    }

    #[cfg(feature = "local")]
    pub fn save_as(&mut self) -> bool {
        let mut d = rfd::FileDialog::new().add_filter("Valve Map", &["vmf"]);
        if let Some(g) = self.game() {
            if !g.map_dir.is_empty() {
                d = d.set_directory(&g.map_dir);
            }
        }
        if let Some(mut p) = d.save_file() {
            if p.extension().is_none() {
                p.set_extension("vmf");
            }
            self.save_to(&p)
        } else {
            false
        }
    }

    /// Browser: the write finishes asynchronously; the document is marked clean when it does.
    #[cfg(feature = "web")]
    fn save_to(&mut self, p: &Path) -> bool {
        self.prepare_save();
        let text = self.doc.map.to_text();
        self.web.start_write(&p.to_string_lossy(), text.into_bytes());
        self.status = format!("Saving {}...", p.display());
        true
    }

    #[cfg(feature = "local")]
    fn save_to(&mut self, p: &Path) -> bool {
        self.prepare_save();
        match self.doc.map.save(p) {
            Ok(_) => {
                self.doc.path = Some(p.to_path_buf());
                self.doc.dirty = false;
                self.settings.last_map = p.display().to_string();
                self.settings.save();
                self.status = format!("Saved {}", p.display());
                true
            }
            Err(e) => {
                self.status = format!("Save failed: {e:#}");
                false
            }
        }
    }

    fn prepare_save(&mut self) {
        // keep the VMF header in sync with the editor state
        self.doc.map.set_viewsetting("nGridSpacing", &format!("{}", self.grid as i64));
        self.doc.map.set_viewsetting("bSnapToGrid", if self.snap { "1" } else { "0" });
        self.doc.map.set_viewsetting("bShowGrid", if self.show_grid { "1" } else { "0" });
        if let Some(vi) = self.doc.map.header.iter_mut().find(|n| n.key.eq_ignore_ascii_case("versioninfo")) {
            if let crate::kv::Value::Block(c) = &mut vi.value {
                if let Some(mv) = c.iter_mut().find(|n| n.key == "mapversion") {
                    let v: i64 = mv.as_str().and_then(|s| s.parse().ok()).unwrap_or(0) + 1;
                    mv.value = crate::kv::Value::Str(v.to_string());
                    self.doc.map.world.set("mapversion", v.to_string());
                }
            }
        }
    }

    pub fn request(&mut self, a: PendingAction) {
        if self.doc.dirty {
            self.win.pending_action = Some(a);
        } else {
            self.perform(a);
        }
    }

    pub fn perform(&mut self, a: PendingAction) {
        match a {
            PendingAction::New => self.new_map(),
            PendingAction::Close => {
                self.new_map();
                self.settings.last_map.clear();
                self.settings.save();
                self.status = "Map closed".into();
            }
            PendingAction::Open(Some(p)) => self.open_path(&p),
            PendingAction::Open(None) => self.open_dialog(),
            PendingAction::Quit => {
                self.settings.save();
                #[cfg(feature = "local")]
                clear_session_marker();
                self.win.quit_confirm = true;
                self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    #[cfg(feature = "local")]
    pub fn run_map(&mut self) {
        // save first (a compile needs the file on disk)
        if self.doc.path.is_none() && !self.save_as() {
            return;
        }
        if self.doc.dirty && !self.save() {
            return;
        }
        let (Some(g), Some(p)) = (self.game().cloned(), self.doc.path.clone()) else {
            self.status = "No game configured".into();
            return;
        };
        let Some(preset) = self.settings.compile.active_preset().cloned() else {
            self.status = "No Run Map preset".into();
            return;
        };
        self.settings.save();
        let ctx = self.ctx.clone();
        self.compile = Some(crate::compile::start(g, preset, p, move || ctx.request_repaint()));
        self.win.compile_log = true;
        self.win.run_map = false;
    }
}
