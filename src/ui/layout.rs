//! Dockable layout: viewports, the tool column and the property tabs are `egui_dock` panes that
//! can be resized (splitters) and dragged around, Hammer style.

use crate::app::{App, VIEW_NAMES};
use eframe::egui::{self, Id, Margin, Sense, Stroke, WidgetText};
use egui_dock::{DockArea, DockState, Node, NodeIndex, Style, TabViewer};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pane {
    /// The 2x2 viewport grid; the view in each cell is `UiLayout::view_kinds[cell]`.
    Views,
    Tools,
    Options,
    /// Legacy: the object editor is a window now. Only kept so older saved layouts still load;
    /// `lenient` strips it.
    Object,
    Textures,
    VisGroups,
}

impl Pane {
    pub const PANELS: [Pane; 4] = [Pane::Tools, Pane::Options, Pane::Textures, Pane::VisGroups];

    pub fn name(self) -> &'static str {
        match self {
            Pane::Tools => "Tools",
            Pane::Options => "Options",
            Pane::Object => "Object",
            Pane::Textures => "Textures",
            Pane::VisGroups => "VisGroups",
            Pane::Views => "Views",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiLayout {
    pub dock: DockState<Pane>,
    /// Which view (index into `VIEW_NAMES`: 0 = 3D, 1..=3 = Top/Front/Side) each viewport slot shows.
    pub view_kinds: [usize; 4],
    /// Position of the shared splitters of the 2x2 grid (x, y), as fractions.
    pub grid_split: [f32; 2],
}

impl Default for UiLayout {
    fn default() -> Self {
        let mut dock = DockState::new(vec![Pane::Views]);
        let tree = dock.main_surface_mut();
        // for left / above splits the fraction is the share of the new node
        let [views, _] = tree.split_right(NodeIndex::root(), 0.78, vec![Pane::Textures, Pane::VisGroups]);
        let [views, _] = tree.split_left(views, 0.04, vec![Pane::Tools]);
        tree.split_above(views, 0.1, vec![Pane::Options]);
        UiLayout { dock, view_kinds: [0, 1, 2, 3], grid_split: [0.5, 0.5] }
    }
}

impl UiLayout {
    /// A layout read from disk is only usable if every viewport slot is still present.
    pub fn is_valid(&self) -> bool {
        self.view_kinds.iter().all(|k| *k < VIEW_NAMES.len()) && [Pane::Views, Pane::Tools, Pane::Options].iter().all(|p| self.dock.find_tab(p).is_some())
    }

    /// Everything that is worth saving, without the per-frame screen rects.
    fn signature(&self) -> String {
        let mut s = format!("{:?}{:?}", self.view_kinds, self.grid_split);
        for (_, node) in self.dock.iter_all_nodes() {
            match node {
                Node::Leaf(l) => s += &format!("|{:?}@{}", l.tabs, l.active.0),
                Node::Horizontal(n) | Node::Vertical(n) => s += &format!("|{:.3}", n.fraction),
                Node::Empty => {}
            }
        }
        s
    }

    pub fn toggle(&mut self, pane: Pane) {
        match self.dock.find_tab(&pane) {
            Some(path) => {
                self.dock.remove_tab(path);
            }
            None => self.dock.push_to_focused_leaf(pane),
        }
    }

    pub fn focus(&mut self, pane: Pane) {
        match self.dock.find_tab(&pane) {
            Some(path) => {
                let _ = self.dock.set_active_tab(path);
            }
            None => self.dock.push_to_focused_leaf(pane),
        }
    }
}

/// Used for `Settings::ui`: a broken or incomplete saved layout falls back to the default
/// instead of discarding the whole settings file.
pub fn lenient<'de, D: Deserializer<'de>>(d: D) -> Result<UiLayout, D::Error> {
    let mut layout = UiLayout::deserialize(d).ok().filter(UiLayout::is_valid).unwrap_or_default();
    layout.dock.retain_tabs(|t| *t != Pane::Object);
    Ok(layout)
}

struct Tabs<'a> {
    app: &'a mut App,
}

impl TabViewer for Tabs<'_> {
    type Tab = Pane;

    fn id(&mut self, tab: &mut Pane) -> Id {
        Id::new(("pane", *tab))
    }

    fn title(&mut self, tab: &mut Pane) -> WidgetText {
        tab.name().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Pane) {
        match *tab {
            Pane::Tools => self.app.tools_pane(ui),
            Pane::Options => self.app.options_pane(ui),
            Pane::Object => {}
            Pane::Textures => self.app.texture_tab(ui),
            Pane::VisGroups => self.app.visgroups_tab(ui),
            Pane::Views => self.app.views_grid(ui),
        }
    }

    fn is_closeable(&self, tab: &Pane) -> bool {
        *tab != Pane::Views
    }

    fn scroll_bars(&self, _tab: &Pane) -> [bool; 2] {
        [false, false]
    }

    fn clear_background(&self, tab: &Pane) -> bool {
        *tab != Pane::Views
    }
}

impl App {
    /// One viewport filling the available space. `kind` is 0 for 3D, 1..=3 for the ortho views.
    pub(crate) fn viewport_ui(&mut self, ui: &mut egui::Ui, kind: usize) {
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        if kind == 0 {
            self.view3d_ui(ui, rect);
        } else {
            self.view2d_ui(ui, rect, kind - 1);
        }
        ui.painter_at(rect).rect_stroke(rect, 0.0, Stroke::new(1.0, egui::Color32::from_gray(70)), egui::StrokeKind::Inside);
    }

    /// The four viewports as one 2x2 scene with shared, draggable splitters.
    pub(crate) fn views_grid(&mut self, ui: &mut egui::Ui) {
        const GAP: f32 = 4.0;
        const MIN: f32 = 0.1;
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        // a maximized view fills this pane only; the other panels stay visible
        if let Some(kind) = self.maximized {
            ui.scope_builder(egui::UiBuilder::new().max_rect(full).id_salt("maximized"), |ui| self.viewport_ui(ui, kind));
            return;
        }
        let [mut fx, mut fy] = self.settings.ui.grid_split;
        let sx = full.left() + full.width() * fx;
        let sy = full.top() + full.height() * fy;
        let vbar = egui::Rect::from_min_max(egui::pos2(sx - GAP / 2.0, full.top()), egui::pos2(sx + GAP / 2.0, full.bottom()));
        let hbar = egui::Rect::from_min_max(egui::pos2(full.left(), sy - GAP / 2.0), egui::pos2(full.right(), sy + GAP / 2.0));
        let rv = ui.interact(vbar, ui.id().with("split_v"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        let rh = ui.interact(hbar, ui.id().with("split_h"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
        if let Some(p) = rv.interact_pointer_pos().filter(|_| rv.dragged()) {
            fx = ((p.x - full.left()) / full.width()).clamp(MIN, 1.0 - MIN);
        }
        if let Some(p) = rh.interact_pointer_pos().filter(|_| rh.dragged()) {
            fy = ((p.y - full.top()) / full.height()).clamp(MIN, 1.0 - MIN);
        }
        self.settings.ui.grid_split = [fx, fy];
        let (sx, sy) = (full.left() + full.width() * fx, full.top() + full.height() * fy);
        for cell in 0..4 {
            let (col, row) = (cell % 2, cell / 2);
            let rect = egui::Rect::from_min_max(
                egui::pos2(if col == 0 { full.left() } else { sx + GAP / 2.0 }, if row == 0 { full.top() } else { sy + GAP / 2.0 }),
                egui::pos2(if col == 0 { sx - GAP / 2.0 } else { full.right() }, if row == 0 { sy - GAP / 2.0 } else { full.bottom() }),
            );
            let kind = self.settings.ui.view_kinds[cell];
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect).id_salt(("cell", cell)), |ui| self.viewport_ui(ui, kind));
            // Hammer-style caption in the cell's top-left corner; click to pick the view
            let caption = egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 4.0), egui::vec2(88.0, 18.0));
            let resp = ui.put(
                caption,
                egui::Button::new(egui::RichText::new(VIEW_NAMES[kind]).small().color(egui::Color32::from_gray(225)))
                    .fill(egui::Color32::from_black_alpha(150))
                    .stroke(Stroke::new(1.0, egui::Color32::from_gray(90))),
            );
            egui::Popup::menu(&resp).show(|ui| {
                for (k, name) in VIEW_NAMES.iter().enumerate() {
                    if ui.selectable_label(self.settings.ui.view_kinds[cell] == k, *name).clicked() {
                        // a view shows in one cell only: swap with the cell that had it
                        if let Some(other) = self.settings.ui.view_kinds.iter().position(|v| *v == k) {
                            self.settings.ui.view_kinds[other] = self.settings.ui.view_kinds[cell];
                        }
                        self.settings.ui.view_kinds[cell] = k;
                        ui.close();
                    }
                }
            });
        }
    }

    /// The central area: the dock, or a single maximized view.
    pub fn central_ui(&mut self, ui: &mut egui::Ui) {
        self.hover_world = None;
        self.view_busy = false;
        self.freelook_drawn = false;
        self.prepare_scene();
        if let Some(p) = self.focus_pane.take() {
            self.settings.ui.focus(p);
        }
        let mut style = Style::from_egui(ui.style());
        style.tab.tab_body.inner_margin = Margin::ZERO;
        style.tab.tab_body.stroke = Stroke::NONE;
        style.main_surface_border_stroke = Stroke::NONE;
        style.separator.extra = 40.0;
        let mut dock = std::mem::replace(&mut self.settings.ui.dock, DockState::new(vec![]));
        DockArea::new(&mut dock).style(style).show_add_buttons(false).show_leaf_collapse_buttons(false).show_leaf_close_all_buttons(false).show_inside(ui, &mut Tabs { app: self });
        self.settings.ui.dock = dock;
        self.save_layout_on_release(ui.ctx());
        if self.freelook && !self.freelook_drawn {
            let ctx = ui.ctx().clone();
            self.set_freelook(&ctx, false);
        }
    }

    /// Persists the layout once a splitter / tab drag ends and the layout actually changed.
    fn save_layout_on_release(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }
        let now = self.settings.ui.signature();
        if now != self.layout_saved {
            self.layout_saved = now;
            self.settings.save();
        }
    }

    pub fn reset_layout(&mut self) {
        self.settings.ui = UiLayout::default();
        self.maximized = None;
    }
}
