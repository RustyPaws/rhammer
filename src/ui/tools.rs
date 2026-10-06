//! Editor tools: their metadata (name, shortcut, icon) and the Tools pane.

use crate::app::App;
use crate::ui::layout::Pane;
use eframe::egui::{self, ImageSource, Key};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Block,
    Entity,
    Clip,
    Vertex,
    Texture,
}

impl Tool {
    pub const ALL: [Tool; 6] = [Tool::Select, Tool::Block, Tool::Entity, Tool::Clip, Tool::Vertex, Tool::Texture];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Selection tool",
            Tool::Block => "Block tool",
            Tool::Entity => "Entity tool",
            Tool::Clip => "Clip tool",
            Tool::Vertex => "Vertex manipulation tool",
            Tool::Texture => "Texture application",
        }
    }

    /// Shift+key activates the tool.
    pub fn shortcut(self) -> Key {
        match self {
            Tool::Select => Key::S,
            Tool::Block => Key::B,
            Tool::Entity => Key::E,
            Tool::Clip => Key::X,
            Tool::Vertex => Key::V,
            Tool::Texture => Key::A,
        }
    }

    /// Icons live in `assets/icons/tools`; they are drawn white and tinted at runtime.
    pub fn icon(self) -> ImageSource<'static> {
        match self {
            Tool::Select => egui::include_image!("../../assets/icons/tools/select.svg"),
            Tool::Block => egui::include_image!("../../assets/icons/tools/block.svg"),
            Tool::Entity => egui::include_image!("../../assets/icons/tools/entity.svg"),
            Tool::Clip => egui::include_image!("../../assets/icons/tools/clip.svg"),
            Tool::Vertex => egui::include_image!("../../assets/icons/tools/vertex.svg"),
            Tool::Texture => egui::include_image!("../../assets/icons/tools/texture.svg"),
        }
    }

    pub fn tooltip(self) -> String {
        format!("{} (Shift+{})", self.name(), self.shortcut().name())
    }
}

/// Side of a (square) tool button and of the icon inside it, in points.
const BUTTON: f32 = 32.0;
const ICON: f32 = 20.0;

impl App {
    /// Switches tools; the texture tool also brings the Textures pane forward.
    pub fn set_tool(&mut self, t: Tool) {
        self.tool = t;
        if t == Tool::Texture {
            self.focus_pane = Some(Pane::Textures);
        }
    }

    /// Small square icon buttons that wrap to fit the pane: a narrow pane gets a column,
    /// a wide one a row (or a grid when neither fits).
    pub fn tools_pane(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for t in Tool::ALL {
                    let active = self.tool == t;
                    let tint = if active { ui.visuals().selection.stroke.color } else { ui.visuals().widgets.inactive.fg_stroke.color };
                    let img = egui::Image::new(t.icon()).fit_to_exact_size(egui::vec2(ICON, ICON)).tint(tint);
                    let button = egui::Button::image(img).selected(active);
                    if ui.add_sized([BUTTON, BUTTON], button).on_hover_text(t.tooltip()).clicked() {
                        self.set_tool(t);
                    }
                }
            });
        });
    }
}
