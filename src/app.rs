//! Application state, window chrome, menus, file operations and hotkeys.

use crate::assets::Materials;
#[cfg(feature = "local")]
use crate::compile::CompileJob;
use crate::config::{GameConfig, Settings};
use crate::editor::doc::{Clipboard, Doc, Sel, Xform};
use crate::formats::fgd::Fgd;
use crate::editor::geom::{self, Plane, Primitive};
use crate::render3d::{Camera, SharedRef};
use crate::formats::vmf::Map;
#[cfg(feature = "local")]
use eframe::egui::Color32;
#[cfg(feature = "web")]
use crate::platform::WebEvent;
use eframe::egui::{self, Align2, Key, RichText};
use glam::{DQuat, DVec3};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Block,
    Entity,
    Clip,
    Texture,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    Object,
    Texture,
    Visgroups,
}

#[derive(Clone, Copy)]
pub struct View2D {
    pub center: (f64, f64),
    pub zoom: f64,
}

pub enum Drag {
    Move { view: usize, start: (f64, f64), delta: (f64, f64), clone: bool },
    Resize { view: usize, hx: i32, hy: i32, orig: (DVec3, DVec3), cur: (f64, f64) },
    BoxSel { view: usize, start: (f64, f64), cur: (f64, f64) },
    BlockNew { view: usize, start: (f64, f64) },
    BlockMove { view: usize, start: (f64, f64), orig: (DVec3, DVec3) },
    BlockResize { view: usize, hx: i32, hy: i32, orig: (DVec3, DVec3) },
    ClipLine { view: usize },
    Rotate { view: usize, center: DVec3, start_angle: f64, angle: f64 },
    /// Dragging the origin marker of the selected entity; `orig` is the origin when the drag began.
    Origin { view: usize, id: u32, orig: DVec3, cur: (f64, f64) },
}

#[derive(Default)]
pub struct ClipState {
    pub view: usize,
    pub p0: Option<(f64, f64)>,
    pub p1: Option<(f64, f64)>,
    /// 0 = keep both, 1 = keep front, 2 = keep back
    pub mode: u8,
}

#[derive(Default)]
pub struct Windows {
    pub game_cfg: bool,
    pub model_viewer: bool,
    #[cfg(feature = "local")]
    pub run_map: bool,
    #[cfg(feature = "local")]
    pub compile_log: bool,
    pub tex_browser: bool,
    pub transform: bool,
    pub find: bool,
    pub about: bool,
    pub map_props: bool,
    pub io_graph: bool,
    pub quit_confirm: bool,
    pub pending_action: Option<PendingAction>,
}

#[derive(Clone)]
pub enum PendingAction {
    New,
    Open(Option<PathBuf>),
    Quit,
}

pub struct TransformDlg {
    pub mv: [f64; 3],
    pub rot: [f64; 3],
    pub scale: [f64; 3],
}

pub struct App {
    pub settings: Settings,
    pub doc: Doc,
    pub fgd: Fgd,
    pub vfs: crate::platform::SharedVfs,
    pub vpks: crate::assets::gamefs::VpkCache,
    /// Browser file access (the same object as `vfs`, with the folder picker).
    #[cfg(feature = "web")]
    pub web: crate::platform::WebFs,
    /// Game data is still being fetched in the browser.
    #[cfg(feature = "web")]
    pub loading: bool,
    /// Remembered game folders that need a click to be re-opened.
    #[cfg(feature = "web")]
    pub locked_roots: Vec<String>,
    /// The "browser not supported" notice was dismissed.
    #[cfg(feature = "web")]
    pub unsupported_ack: bool,
    /// Showing the "copy the game out of system folders" notice before the folder picker.
    #[cfg(feature = "web")]
    pub pick_info: bool,
    #[cfg(feature = "web")]
    pub pick_info_skip: bool,
    pub mats: Materials,
    pub sel: Sel,
    pub sel_stamp: u64,
    pub faces: BTreeSet<u32>,
    pub tool: Tool,
    pub views: [View2D; 3],
    pub cam: Camera,
    pub shared: SharedRef,
    pub world_key: (u64, usize),
    pub overlay_key: (u64, u64, u64),
    pub grid: f64,
    pub snap: bool,
    pub tex_lock: bool,
    pub show_grid: bool,
    pub wireframe: bool,
    pub cur_mat: String,
    pub primitive: Primitive,
    pub prim_sides: usize,
    pub block: Option<(DVec3, DVec3)>,
    pub ent_class: String,
    pub ent_filter: String,
    pub clip: ClipState,
    pub drag: Option<Drag>,
    pub clipboard: Clipboard,
    pub status: String,
    pub maximized: Option<usize>,
    pub tab: RightTab,
    pub win: Windows,
    #[cfg(feature = "local")]
    pub compile: Option<CompileJob>,
    pub tex_filter: String,
    pub cfg_sel: usize,
    pub hover_world: Option<DVec3>,
    pub new_visgroup: String,
    pub find_text: String,
    pub io_graph: crate::ui::io_graph::IoGraph,
    pub transform_dlg: TransformDlg,
    pub hollow_thickness: f64,
    pub last_prop_edit: Option<(String, web_time::Instant)>,
    pub tex_scale: f64,
    pub face_edit: FaceEdit,
    pub ctx: egui::Context,
    pub thumb_tex: std::collections::HashMap<String, egui::TextureHandle>,
    pub rot_mode: bool,
    pub paste_count: i32,
    pub show_entity_names: bool,
    pub inst: std::collections::HashMap<u32, crate::editor::instances::InstGeo>,
    pub inst_cache: crate::editor::instances::InstCache,
    pub inst_key: u64,
    pub models: std::collections::HashMap<String, Option<std::rc::Rc<crate::assets::mdl::Model>>>,
    pub model_budget: i32,
    pub model_starved: bool,
    pub models_key: u64,
    /// Play model animations in the 3D view.
    pub anim_play: bool,
    /// Animation clock in seconds.
    pub anim_time: f64,
    /// Some visible model shows a sequence with more than one frame.
    pub anim_active: bool,
    /// Editor-only sequence override per entity (not saved to the map).
    pub anim_preview: std::collections::HashMap<u32, usize>,
    pub anim_stamp: u64,
    pub anim_key: (u64, usize, u64, u64),
    pub mv: crate::ui::model_viewer::ModelViewer,
}

#[derive(Clone)]
pub struct FaceEdit {
    pub uscale: f64,
    pub vscale: f64,
    pub ushift: f64,
    pub vshift: f64,
    pub rotation: f64,
    pub lightmap: i32,
}

impl Default for FaceEdit {
    fn default() -> Self {
        FaceEdit { uscale: 0.25, vscale: 0.25, ushift: 0.0, vshift: 0.0, rotation: 0.0, lightmap: 16 }
    }
}

pub fn axes(view: usize) -> (usize, usize, usize) {
    match view {
        0 => (0, 1, 2), // top: x right, y up
        1 => (0, 2, 1), // front: x right, z up
        _ => (1, 2, 0), // side: y right, z up
    }
}

pub const VIEW_NAMES: [&str; 4] = ["3D Perspective", "Top (X/Y)", "Front (X/Z)", "Side (Y/Z)"];

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        let mut settings = Settings::load();
        #[cfg(feature = "local")]
        let vfs = crate::platform::default_vfs();
        #[cfg(feature = "web")]
        let web = crate::platform::WebFs::new();
        #[cfg(feature = "web")]
        let vfs: crate::platform::SharedVfs = std::rc::Rc::new(web.clone());
        #[cfg(feature = "web")]
        web.set_ctx(cc.egui_ctx.clone());
        let vpks = crate::assets::gamefs::VpkCache::default();
        let (fgd, mats) = load_game_data(&vfs, &vpks, settings.active_game());
        let shared: SharedRef = Default::default();
        let _ = &mut settings;
        let mut app = App {
            settings,
            doc: Doc::new(Map::new_empty(), None),
            fgd,
            vfs: vfs.clone(),
            vpks,
            #[cfg(feature = "web")]
            web,
            #[cfg(feature = "web")]
            loading: false,
            #[cfg(feature = "web")]
            locked_roots: vec![],
            #[cfg(feature = "web")]
            unsupported_ack: false,
            #[cfg(feature = "web")]
            pick_info: false,
            #[cfg(feature = "web")]
            pick_info_skip: false,
            mats,
            sel: Sel::new(),
            sel_stamp: 0,
            faces: BTreeSet::new(),
            tool: Tool::Select,
            views: [View2D { center: (0.0, 0.0), zoom: 0.5 }; 3],
            cam: Camera::default(),
            shared,
            world_key: (u64::MAX, 0),
            overlay_key: (u64::MAX, 0, 0),
            grid: 16.0,
            snap: true,
            tex_lock: true,
            show_grid: true,
            wireframe: false,
            cur_mat: "dev/dev_measuregeneric01b".into(),
            primitive: Primitive::Block,
            prim_sides: 16,
            block: None,
            ent_class: "info_player_start".into(),
            ent_filter: String::new(),
            clip: ClipState::default(),
            drag: None,
            clipboard: Clipboard::default(),
            status: "Ready".into(),
            maximized: None,
            tab: RightTab::Object,
            win: Windows::default(),
            #[cfg(feature = "local")]
            compile: None,
            tex_filter: String::new(),
            cfg_sel: 0,
            hover_world: None,
            new_visgroup: String::new(),
            find_text: String::new(),
            io_graph: Default::default(),
            transform_dlg: TransformDlg { mv: [0.0; 3], rot: [0.0; 3], scale: [1.0; 3] },
            hollow_thickness: 16.0,
            last_prop_edit: None,
            tex_scale: 0.25,
            face_edit: FaceEdit::default(),
            ctx: cc.egui_ctx.clone(),
            thumb_tex: Default::default(),
            rot_mode: false,
            paste_count: 0,
            show_entity_names: true,
            inst: Default::default(),
            inst_cache: crate::editor::instances::InstCache::new(vfs.clone()),
            inst_key: u64::MAX,
            models: Default::default(),
            model_budget: 0,
            model_starved: false,
            models_key: u64::MAX,
            mv: Default::default(),
            anim_play: false,
            anim_time: 0.0,
            anim_active: false,
            anim_preview: Default::default(),
            anim_stamp: 0,
            anim_key: (u64::MAX, 0, 0, 0),
        };
        if let Some(g) = app.settings.active_game() {
            app.tex_scale = g.default_texture_scale;
            if !g.default_point_entity.is_empty() {
                app.ent_class = g.default_point_entity.clone();
            }
        }
        #[cfg(feature = "web")]
        {
            app.web.start_restore();
            if app.settings.games.is_empty() {
                app.status = "Use File > Open game folder... to load a game".into();
            }
        }
        // open a map given on the command line, or the last one
        #[cfg(feature = "local")]
        {
            let arg = std::env::args().nth(1).map(PathBuf::from);
            let last = (!app.settings.last_map.is_empty()).then(|| PathBuf::from(&app.settings.last_map));
            if let Some(p) = arg.or(last) {
                if p.exists() {
                    app.open_path(&p);
                }
            }
        }
        app
    }

    pub fn game(&self) -> Option<&GameConfig> {
        self.settings.active_game()
    }

    pub fn reload_game_data(&mut self) {
        let (fgd, mats) = load_game_data(&self.vfs, &self.vpks, self.settings.active_game());
        self.fgd = fgd;
        self.mats = mats;
        self.thumb_tex.clear();
        self.inst_cache.clear();
        self.models.clear();
        self.mv = Default::default();
        self.doc.model_bounds.clear();
        self.inst_key = u64::MAX;
        self.world_key = (u64::MAX, 0);
        if let Some(g) = self.settings.active_game() {
            self.tex_scale = g.default_texture_scale;
        }
        // in the browser the files arrive asynchronously: repeat until a pass fetches nothing new
        #[cfg(feature = "web")]
        {
            self.loading = self.vfs.pending() > 0;
        }
    }

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
    fn web_poll(&mut self, ctx: &egui::Context) {
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

    pub fn reset_derived(&mut self) {
        self.inst.clear();
        self.inst_key = u64::MAX;
        self.models_key = u64::MAX;
        self.world_key = (u64::MAX, 0);
        self.overlay_key = (u64::MAX, 0, 0);
        self.anim_key = (u64::MAX, 0, 0, 0);
    }

    pub fn bump_sel(&mut self) {
        self.sel_stamp += 1;
    }

    pub fn set_sel(&mut self, s: Sel) {
        self.sel = s;
        self.rot_mode = false;
        self.bump_sel();
    }

    pub fn snapv(&self, v: f64) -> f64 {
        if self.snap {
            (v / self.grid).round() * self.grid
        } else {
            v
        }
    }

    pub fn title(&self) -> String {
        let name = self.doc.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or("untitled".into());
        format!("{}{} - rhammer", name, if self.doc.dirty { "*" } else { "" })
    }

    // ---- file operations -------------------------------------------------------------------

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
    fn open_map_bytes(&mut self, bytes: &[u8], p: &Path) {
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
            PendingAction::Open(Some(p)) => self.open_path(&p),
            PendingAction::Open(None) => self.open_dialog(),
            PendingAction::Quit => {
                self.settings.save();
                self.win.quit_confirm = true;
                self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    // ---- camera / view helpers ---------------------------------------------------------------

    pub fn frame_all(&mut self) {
        let ids = self.doc.all_ids();
        let all: Sel = ids.into_iter().collect();
        self.frame_sel_set(&all);
    }

    pub fn frame_selection(&mut self) {
        let s = self.sel.clone();
        self.frame_sel_set(&s);
    }

    fn frame_sel_set(&mut self, s: &Sel) {
        let Some((min, max)) = self.doc.sel_bounds(s, &self.fgd) else { return };
        let c = (min + max) * 0.5;
        let size = (max - min).max(DVec3::splat(64.0));
        for (vi, v) in self.views.iter_mut().enumerate() {
            let (u, w, _) = axes(vi);
            v.center = (c[u], c[w]);
            v.zoom = (600.0 / size[u].max(size[w])).clamp(0.02, 8.0);
        }
        let r = (max - min).length().max(128.0);
        self.cam.pos = c + DVec3::new(-r * 0.8, -r * 0.8, r * 0.6);
        let d = (c - self.cam.pos).normalize();
        self.cam.yaw = d.y.atan2(d.x).to_degrees();
        self.cam.pitch = d.z.asin().to_degrees();
    }

    // ---- editing commands ------------------------------------------------------------------

    pub fn delete_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let s = self.sel.clone();
        self.doc.delete(&s);
        self.set_sel(Sel::new());
        self.status = format!("Deleted {} objects", s.len());
    }

    pub fn copy_selection(&mut self) {
        self.clipboard = self.doc.copy_objects(&self.sel);
        self.paste_count = 0;
        self.status = format!("Copied {} objects", self.sel.len());
    }

    pub fn cut_selection(&mut self) {
        self.copy_selection();
        self.delete_selection();
    }

    pub fn paste_clipboard(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.paste_count += 1;
        let off = DVec3::splat(self.grid * self.paste_count as f64);
        let off = DVec3::new(off.x, -off.y, 0.0);
        self.doc.checkpoint();
        let cb = self.clipboard.clone();
        let n = self.doc.paste(&cb, off);
        self.set_sel(n);
        self.status = "Pasted".into();
    }

    pub fn duplicate_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let s = self.sel.clone();
        let n = self.doc.clone_objects(&s, DVec3::new(self.grid, -self.grid, 0.0));
        self.set_sel(n);
    }

    pub fn apply_xform(&mut self, xf: Xform) {
        if self.sel.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let s = self.sel.clone();
        self.doc.transform(&s, &xf, self.tex_lock);
    }

    pub fn rotate_selection(&mut self, axis: DVec3, deg: f64) {
        let Some((a, b)) = self.doc.sel_bounds(&self.sel, &self.fgd) else { return };
        let c = (a + b) * 0.5;
        let c = DVec3::new(self.snapv(c.x), self.snapv(c.y), self.snapv(c.z));
        self.apply_xform(Xform::Rotate { center: c, q: DQuat::from_axis_angle(axis, deg.to_radians()) });
    }

    pub fn mirror_selection(&mut self, axis: usize) {
        let Some((a, b)) = self.doc.sel_bounds(&self.sel, &self.fgd) else { return };
        let c = (a + b) * 0.5;
        self.apply_xform(Xform::Mirror { center: c, axis });
    }

    pub fn hollow_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.doc.checkpoint();
        let s = self.sel.clone();
        let n = self.doc.hollow(&s, self.hollow_thickness, self.tex_scale, 16);
        if !n.is_empty() {
            self.set_sel(n);
        }
    }

    pub fn tie_selection(&mut self, class: &str) {
        let s = self.sel.clone();
        self.doc.checkpoint();
        if let Some(id) = self.doc.tie_to_entity(&s, class, &self.fgd) {
            self.set_sel([id].into_iter().collect());
            self.tab = RightTab::Object;
        }
    }

    pub fn move_to_world(&mut self) {
        let s = self.sel.clone();
        self.doc.checkpoint();
        let n = self.doc.move_to_world(&s);
        self.set_sel(n);
    }

    pub fn commit_block(&mut self) {
        let Some((mut a, mut b)) = self.block else { return };
        for i in 0..3 {
            if a[i] > b[i] {
                std::mem::swap(&mut a[i], &mut b[i]);
            }
        }
        let planes = geom::primitive_planes(self.primitive, a, b, self.prim_sides);
        self.doc.checkpoint();
        let lm = self.game().map(|g| g.default_lightmap_scale).unwrap_or(16);
        if let Some(id) = self.doc.create_solid(&planes, &self.cur_mat.clone(), self.tex_scale, lm) {
            self.set_sel([id].into_iter().collect());
            self.block = None;
            self.status = format!("Created {}", self.primitive.name());
        } else {
            self.status = "Could not create brush (zero size?)".into();
        }
    }

    pub fn commit_clip(&mut self) {
        let (Some(p0), Some(p1)) = (self.clip.p0, self.clip.p1) else { return };
        if (p0.0 - p1.0).abs() < 1e-6 && (p0.1 - p1.1).abs() < 1e-6 {
            return;
        }
        let (ua, va, wa) = axes(self.clip.view);
        let mut a = DVec3::ZERO;
        let mut b = DVec3::ZERO;
        a[ua] = p0.0;
        a[va] = p0.1;
        b[ua] = p1.0;
        b[va] = p1.1;
        let mut wdir = DVec3::ZERO;
        wdir[wa] = 1.0;
        // normal perpendicular to the line within the view plane
        let n = (b - a).cross(wdir);
        if n.length() < 1e-9 {
            return;
        }
        let n = n.normalize();
        let pl = Plane { n, d: n.dot(a) };
        let (kf, kb) = match self.clip.mode {
            1 => (true, false),
            2 => (false, true),
            _ => (true, true),
        };
        self.doc.checkpoint();
        let s = self.sel.clone();
        let mat = self.cur_mat.clone();
        let n = self.doc.clip(&s, &pl, kf, kb, &mat);
        self.set_sel(n);
        self.clip.p0 = None;
        self.clip.p1 = None;
    }

    pub fn undo(&mut self) {
        if self.doc.undo() {
            self.sel.retain(|id| self.doc.index.contains_key(id));
            self.bump_sel();
            self.status = "Undo".into();
        }
    }

    pub fn redo(&mut self) {
        if self.doc.redo() {
            self.sel.retain(|id| self.doc.index.contains_key(id));
            self.bump_sel();
            self.status = "Redo".into();
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
        self.settings.save();
        let ctx = self.ctx.clone();
        self.compile = Some(crate::compile::start(g, self.settings.compile.clone(), p, move || ctx.request_repaint()));
        self.win.compile_log = true;
        self.win.run_map = false;
    }

    // ---- hotkeys -------------------------------------------------------------------------

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || ctx.input(|i| i.pointer.secondary_down()) {
            return; // typing, or flying the 3D camera with RMB held
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
            if pressed(Key::D) {
                self.duplicate_selection();
            }
            if pressed(Key::H) {
                self.hollow_selection();
            }
            if pressed(Key::M) {
                self.win.transform = true;
            }
            if pressed(Key::F) {
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
            if pressed(Key::S) { self.tool = Tool::Select; }
            if pressed(Key::B) { self.tool = Tool::Block; }
            if pressed(Key::E) { self.tool = Tool::Entity; }
            if pressed(Key::C) { self.tool = Tool::Clip; }
            if pressed(Key::A) { self.tool = Tool::Texture; self.tab = RightTab::Texture; }
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
                if !self.sel.is_empty() {
                    self.set_sel(Sel::new());
                }
                self.faces.clear();
                self.bump_sel();
            }
            if pressed(Key::Enter) {
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

    pub fn default_solid_class(&self) -> String {
        self.game().map(|g| g.default_solid_entity.clone()).filter(|s| !s.is_empty()).unwrap_or("func_detail".into())
    }

    // ---- UI -------------------------------------------------------------------------------

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
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
            });
            ui.menu_button("Edit", |ui| {
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
                if ui.button("Map properties...").clicked() {
                    self.win.map_props = true;
                    ui.close();
                }
            });
            ui.menu_button("Tools", |ui| {
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
            });
            ui.menu_button("View", |ui| {
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
            });
            ui.menu_button("Map", |ui| {
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
            });
            ui.menu_button("Options", |ui| {
                if ui.button("Game configurations...").clicked() {
                    self.win.game_cfg = true;
                    self.cfg_sel = self.settings.active;
                    ui.close();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About").clicked() {
                    self.win.about = true;
                    ui.close();
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (t, label, tip) in [
                (Tool::Select, "Select", "Selection tool (Shift+S)"),
                (Tool::Block, "Block", "Block tool (Shift+B)"),
                (Tool::Entity, "Entity", "Entity tool (Shift+E)"),
                (Tool::Clip, "Clip", "Clip tool (Shift+C)"),
                (Tool::Texture, "Texture", "Texture application (Shift+A)"),
            ] {
                if ui.selectable_label(self.tool == t, label).on_hover_text(tip).clicked() {
                    self.tool = t;
                    if t == Tool::Texture {
                        self.tab = RightTab::Texture;
                    }
                }
            }
            ui.separator();
            ui.label("Grid:");
            egui::ComboBox::from_id_salt("grid").selected_text(format!("{}", self.grid)).width(60.0).show_ui(ui, |ui| {
                for g in [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0] {
                    ui.selectable_value(&mut self.grid, g, format!("{g}"));
                }
            });
            ui.checkbox(&mut self.snap, "Snap");
            ui.checkbox(&mut self.show_grid, "Grid");
            ui.checkbox(&mut self.tex_lock, "Texture lock");
            ui.separator();
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
                    ui.label(format!("Material: {}", self.cur_mat));
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
                    ui.label(format!("Material: {}", self.cur_mat));
                    ui.label(RichText::new("LMB select face - RMB apply texture").weak());
                }
                Tool::Select => {
                    ui.label(RichText::new("click / drag to select - Shift+drag clones").weak());
                }
            }
        });
    }

    pub fn entity_class_picker(&mut self, ui: &mut egui::Ui) {
        let cur = self.ent_class.clone();
        egui::ComboBox::from_id_salt("entclass").selected_text(cur).width(220.0).height(400.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show_ui(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.ent_filter).hint_text("filter..."));
            let f = self.ent_filter.to_ascii_lowercase();
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                let names: Vec<String> = self
                    .fgd
                    .point_classes()
                    .map(|c| c.name.clone())
                    .filter(|n| f.is_empty() || n.to_ascii_lowercase().contains(&f))
                    .collect();
                for n in names {
                    if ui.selectable_label(self.ent_class == n, &n).clicked() {
                        self.ent_class = n;
                        ui.close();
                    }
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
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

fn load_game_data(vfs: &crate::platform::SharedVfs, vpks: &crate::assets::gamefs::VpkCache, g: Option<&GameConfig>) -> (Fgd, Materials) {
    let Some(g) = g else {
        return (Fgd::default(), Materials::new(vfs.clone(), vpks, Path::new("")));
    };
    let mut fgd = Fgd::default();
    if let Some(first) = g.fgds.first() {
        fgd = Fgd::load(&**vfs, Path::new(first));
        // additional FGDs are merged by loading them as includes
        if g.fgds.len() > 1 {
            // Simple approach: build a temporary include-all file in memory is not possible, so merge classes.
            for extra in &g.fgds[1..] {
                let e = Fgd::load(&**vfs, Path::new(extra));
                for (k, v) in e.classes {
                    fgd.classes.insert(k, v);
                }
                for n in e.names {
                    if !fgd.names.contains(&n) {
                        fgd.names.push(n);
                    }
                }
            }
            fgd.names.sort_by_key(|s| s.to_ascii_lowercase());
        }
    }
    let mats = Materials::new(vfs.clone(), vpks, &g.game_path());
    (fgd, mats)
}

impl App {
    /// Developer aid: RHAMMER_SHOT=<file.png> saves a screenshot after a few frames and exits.
    #[cfg(feature = "local")]
    fn debug_screenshot(&mut self, ctx: &egui::Context) {
        let Ok(path) = std::env::var("RHAMMER_SHOT") else { return };
        let frames: u64 = std::env::var("RHAMMER_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
        let n = ctx.cumulative_pass_nr();
        ctx.request_repaint();
        if n == frames / 2 {
            if let Ok(s) = std::env::var("RHAMMER_SELECT_ALL") {
                if s == "1" {
                    let all: Sel = self.doc.all_ids().into_iter().collect();
                    self.set_sel(all);
                }
            }
            if let Ok(t) = std::env::var("RHAMMER_TAB") {
                self.tab = match t.as_str() {
                    "tex" => RightTab::Texture,
                    "vis" => RightTab::Visgroups,
                    _ => RightTab::Object,
                };
            }
            if let Ok(id) = std::env::var("RHAMMER_PICK") {
                if let Ok(id) = id.parse::<u32>() {
                    self.set_sel([id].into_iter().collect());
                }
            }
            if std::env::var("RHAMMER_FRAME").is_ok() {
                self.frame_selection();
            }
            if let Ok(m) = std::env::var("RHAMMER_MAX") {
                self.maximized = m.parse().ok();
            }
            if let Ok(c) = std::env::var("RHAMMER_CAM") {
                let v: Vec<f64> = c.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if v.len() == 5 {
                    self.cam.pos = DVec3::new(v[0], v[1], v[2]);
                    self.cam.yaw = v[3];
                    self.cam.pitch = v[4];
                }
            }
            if std::env::var("RHAMMER_WIN").ok().as_deref() == Some("cfg") {
                self.win.game_cfg = true;
            }
        }
        if n == frames {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let mut buf = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                buf.extend_from_slice(&p.to_array());
            }
            let _ = image_save(&path, w as u32, h as u32, &buf);
            std::process::exit(0);
        }
    }
}

/// Minimal uncompressed PNG writer (no extra dependency).
#[cfg(feature = "local")]
fn image_save(path: &str, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = ty.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for y in 0..h as usize {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * w as usize * 4..(y + 1) * w as usize * 4]);
    }
    // zlib stored blocks
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        z.push(if chunks.peek().is_none() { 1 } else { 0 });
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::File::create(path)?.write_all(&out)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.title()));

        if ctx.input(|i| i.viewport().close_requested()) && self.doc.dirty && !self.win.quit_confirm {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.win.pending_action = Some(PendingAction::Quit);
        }
        #[cfg(feature = "web")]
        self.web_poll(&ctx);
        self.handle_keys(&ctx);
        #[cfg(feature = "local")]
        if let Some(job) = &mut self.compile {
            if job.poll() {
                ctx.request_repaint();
            }
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::right("right").default_size(330.0).min_size(260.0).resizable(true).show(ui, |ui| self.right_panel(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.views_ui(ui));

        self.dialogs(&ctx);
        let _ = Align2::CENTER_CENTER;
        #[cfg(feature = "local")]
        self.debug_screenshot(&ctx);
    }
}
