//! Application state and the per-frame loop. The rest of `App` lives in the submodules and in
//! `crate::ui`.

mod commands;
#[cfg(feature = "local")]
mod debug;
mod files;
mod gamedata;
mod hotkeys;
#[cfg(feature = "web")]
mod web;

use crate::assets::Materials;
#[cfg(feature = "local")]
use crate::compile::CompileJob;
use crate::config::{GameConfig, Settings};
use crate::editor::doc::{Clipboard, Doc, Sel};
use crate::formats::fgd::Fgd;
use crate::editor::geom::Primitive;
use crate::ui::layout::Pane;
use crate::render3d::{Camera, SharedRef};
use crate::formats::vmf::Map;
use eframe::egui;
use glam::DVec3;
use std::collections::BTreeSet;
use std::path::PathBuf;
#[cfg(feature = "local")]
use std::path::Path;
#[cfg(feature = "local")]
use files::{clear_session_marker, SESSION_MARKER};
use gamedata::load_game_data;

pub use crate::ui::tools::Tool;

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
    /// Moving the selected corners of a brush. `view` 3 is the 3D view; `plane` holds the two world
    /// axes that move, `start` is the raw mouse position in world space and `grab` the corner under it.
    Vertex { view: usize, plane: [usize; 2], start: DVec3, grab: DVec3, delta: DVec3 },
}

/// Which pieces of a clipped brush stay (white in the preview) or go (red).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ClipMode {
    /// Split the brush, keep both pieces.
    #[default]
    Both,
    /// Keep the side the arrow points to.
    KeepFront,
    KeepBack,
}

impl ClipMode {
    pub const ALL: [ClipMode; 3] = [ClipMode::Both, ClipMode::KeepFront, ClipMode::KeepBack];

    pub fn label(self) -> &'static str {
        match self {
            ClipMode::Both => "Split (keep both)",
            ClipMode::KeepFront => "Keep front, remove back",
            ClipMode::KeepBack => "Keep back, remove front",
        }
    }

    pub fn next(self) -> ClipMode {
        match self {
            ClipMode::Both => ClipMode::KeepFront,
            ClipMode::KeepFront => ClipMode::KeepBack,
            ClipMode::KeepBack => ClipMode::Both,
        }
    }

    /// (keep front, keep back)
    pub fn keeps(self) -> (bool, bool) {
        match self {
            ClipMode::Both => (true, true),
            ClipMode::KeepFront => (true, false),
            ClipMode::KeepBack => (false, true),
        }
    }
}

#[derive(Default)]
pub struct ClipState {
    pub view: usize,
    pub p0: Option<(f64, f64)>,
    pub p1: Option<(f64, f64)>,
    pub mode: ClipMode,
}

#[derive(Default)]
pub struct Windows {
    pub game_cfg: bool,
    pub editor_opts: bool,
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
    pub object_props: bool,
    pub quit_confirm: bool,
    pub pending_action: Option<PendingAction>,
}

#[derive(Clone)]
pub enum PendingAction {
    New,
    Close,
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
    pub vtx: crate::ui::views::VertexState,
    pub drag: Option<Drag>,
    pub clipboard: Clipboard,
    pub status: String,
    pub maximized: Option<usize>,
    /// Mouse-look is on (cursor locked) in the 3D view.
    pub freelook: bool,
    /// The 3D view was drawn this frame (freelook ends when it is not).
    pub freelook_drawn: bool,
    /// When the fly speed last changed, for the on-screen readout.
    pub speed_shown: Option<web_time::Instant>,
    /// Pane to bring to the front on the next frame (e.g. Textures when the Texture tool is picked).
    pub focus_pane: Option<Pane>,
    /// Serialized layout last written to disk.
    pub layout_saved: String,
    pub win: Windows,
    #[cfg(feature = "local")]
    pub compile: Option<CompileJob>,
    pub tex_filter: String,
    pub cfg_sel: usize,
    /// Draft of `settings.editor.steam_dir` while the Editor options window is open.
    pub steam_edit: String,
    pub hover_world: Option<DVec3>,
    /// Something is being dragged in a viewport this frame (moving, drawing, panning, looking).
    pub view_busy: bool,
    pub new_visgroup: String,
    pub find_text: String,
    pub io_graph: crate::ui::io_graph::IoGraph,
    /// Open tab of the Object Properties window.
    pub obj_tab: crate::ui::props::ObjTab,
    pub transform_dlg: TransformDlg,
    pub hollow_thickness: f64,
    pub last_prop_edit: Option<(String, web_time::Instant)>,
    pub tex_scale: f64,
    pub face_edit: FaceEdit,
    pub ctx: egui::Context,
    pub thumb_tex: std::collections::HashMap<String, egui::TextureHandle>,
    /// Entity icon sprites by material name; `None` when the material has no usable texture.
    pub sprite_tex: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    pub sprites_key: u64,
    pub sprites_pending: bool,
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
        egui_extras::install_image_loaders(&cc.egui_ctx);
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
        let (fgd, mats) = load_game_data(&vfs, &vpks, settings.active_game(), settings.steam_dir().as_deref());
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
            freelook: false,
            freelook_drawn: false,
            speed_shown: None,
            focus_pane: None,
            layout_saved: String::new(),
            win: Windows::default(),
            #[cfg(feature = "local")]
            compile: None,
            tex_filter: String::new(),
            cfg_sel: 0,
            steam_edit: String::new(),
            hover_world: None,
            view_busy: false,
            new_visgroup: String::new(),
            find_text: String::new(),
            io_graph: Default::default(),
            obj_tab: Default::default(),
            transform_dlg: TransformDlg { mv: [0.0; 3], rot: [0.0; 3], scale: [1.0; 3] },
            hollow_thickness: 16.0,
            last_prop_edit: None,
            tex_scale: 0.25,
            face_edit: FaceEdit::default(),
            ctx: cc.egui_ctx.clone(),
            thumb_tex: Default::default(),
            sprite_tex: Default::default(),
            sprites_key: u64::MAX,
            sprites_pending: false,
            rot_mode: false,
            vtx: Default::default(),
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
            // the last map is reopened only after a crash (marker left behind) or when the option is on
            let crashed = Path::new(SESSION_MARKER).exists();
            let _ = std::fs::write(SESSION_MARKER, "");
            let restore = crashed || app.settings.editor.restore_last_map;
            let last = (restore && !app.settings.last_map.is_empty()).then(|| PathBuf::from(&app.settings.last_map));
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
        self.vtx.sel.clear();
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

}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.title()));

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.doc.dirty && !self.win.quit_confirm {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.win.pending_action = Some(PendingAction::Quit);
            } else {
                #[cfg(feature = "local")]
                clear_session_marker();
            }
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
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.central_ui(ui));

        self.dialogs(&ctx);
        #[cfg(feature = "local")]
        self.debug_screenshot(&ctx);
    }
}
