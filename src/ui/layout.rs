//! Dockable layout: viewports, the tool column and the property tabs are `egui_dock` panes that
//! can be resized (splitters) and dragged around, Hammer style.

use crate::app::{App, VIEW_NAMES};
use eframe::egui::{self, Id, Margin, Sense, Stroke, WidgetText};
use egui_dock::{DockArea, DockState, Node, NodeIndex, Style, TabViewer};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pane {
    /// Viewport slot 0..=3; the view it shows is `UiLayout::view_kinds[slot]`.
    View0,
    View1,
    View2,
    View3,
    Tools,
    Object,
    Textures,
    VisGroups,
}

impl Pane {
    pub const VIEWS: [Pane; 4] = [Pane::View0, Pane::View1, Pane::View2, Pane::View3];
    pub const PANELS: [Pane; 4] = [Pane::Tools, Pane::Object, Pane::Textures, Pane::VisGroups];

    fn slot(self) -> Option<usize> {
        Self::VIEWS.iter().position(|p| *p == self)
    }

    pub fn name(self) -> &'static str {
        match self {
            Pane::Tools => "Tools",
            Pane::Object => "Object",
            Pane::Textures => "Textures",
            Pane::VisGroups => "VisGroups",
            _ => "View",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiLayout {
    pub dock: DockState<Pane>,
    /// Which view (index into `VIEW_NAMES`: 0 = 3D, 1..=3 = Top/Front/Side) each viewport slot shows.
    pub view_kinds: [usize; 4],
}

impl Default for UiLayout {
    fn default() -> Self {
        let mut dock = DockState::new(vec![Pane::View0]);
        let tree = dock.main_surface_mut();
        let [tl, tr] = tree.split_right(NodeIndex::root(), 0.5, vec![Pane::View1]);
        tree.split_below(tl, 0.5, vec![Pane::View2]);
        tree.split_below(tr, 0.5, vec![Pane::View3]);
        let [rest, _] = tree.split_right(NodeIndex::root(), 0.75, vec![Pane::Object, Pane::Textures, Pane::VisGroups]);
        // for a left split the fraction is the share of the new (left) node
        tree.split_left(rest, 0.14, vec![Pane::Tools]);
        UiLayout { dock, view_kinds: [0, 1, 2, 3] }
    }
}

impl UiLayout {
    /// A layout read from disk is only usable if every viewport slot is still present.
    pub fn is_valid(&self) -> bool {
        self.view_kinds.iter().all(|k| *k < VIEW_NAMES.len()) && Pane::VIEWS.iter().all(|p| self.dock.find_tab(p).is_some())
    }

    /// Everything that is worth saving, without the per-frame screen rects.
    fn signature(&self) -> String {
        let mut s = format!("{:?}", self.view_kinds);
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
    Ok(UiLayout::deserialize(d).ok().filter(UiLayout::is_valid).unwrap_or_default())
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
        match tab.slot() {
            Some(slot) => VIEW_NAMES[self.app.settings.ui.view_kinds[slot]].into(),
            None => tab.name().into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Pane) {
        match *tab {
            Pane::Tools => self.app.tools_pane(ui),
            Pane::Object => self.app.object_tab(ui),
            Pane::Textures => self.app.texture_tab(ui),
            Pane::VisGroups => self.app.visgroups_tab(ui),
            view => {
                let kind = self.app.settings.ui.view_kinds[view.slot().unwrap_or(0)];
                self.app.viewport_ui(ui, kind);
            }
        }
    }

    /// Clicking a viewport's caption picks the view it shows.
    fn on_tab_button(&mut self, tab: &mut Pane, response: &egui::Response) {
        let Some(slot) = tab.slot() else { return };
        egui::Popup::menu(response).show(|ui| {
            for (kind, name) in VIEW_NAMES.iter().enumerate() {
                if ui.selectable_label(self.app.settings.ui.view_kinds[slot] == kind, *name).clicked() {
                    self.app.settings.ui.view_kinds[slot] = kind;
                    ui.close();
                }
            }
        });
    }

    fn is_closeable(&self, tab: &Pane) -> bool {
        tab.slot().is_none()
    }

    fn scroll_bars(&self, _tab: &Pane) -> [bool; 2] {
        [false, false]
    }

    fn clear_background(&self, tab: &Pane) -> bool {
        tab.slot().is_none()
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

    /// The central area: the dock, or a single maximized view.
    pub fn central_ui(&mut self, ui: &mut egui::Ui) {
        self.hover_world = None;
        self.prepare_scene();
        if let Some(kind) = self.maximized {
            let full = ui.available_rect_before_wrap();
            ui.scope_builder(egui::UiBuilder::new().max_rect(full), |ui| self.viewport_ui(ui, kind));
            return;
        }
        if let Some(p) = self.focus_pane.take() {
            self.settings.ui.focus(p);
        }
        let mut style = Style::from_egui(ui.style());
        style.tab.tab_body.inner_margin = Margin::ZERO;
        style.tab.tab_body.stroke = Stroke::NONE;
        style.main_surface_border_stroke = Stroke::NONE;
        let mut dock = std::mem::replace(&mut self.settings.ui.dock, DockState::new(vec![]));
        DockArea::new(&mut dock).style(style).show_add_buttons(false).show_leaf_collapse_buttons(false).show_leaf_close_all_buttons(false).show_inside(ui, &mut Tabs { app: self });
        self.settings.ui.dock = dock;
        self.save_layout_on_release(ui.ctx());
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
