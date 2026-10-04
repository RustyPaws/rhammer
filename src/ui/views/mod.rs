//! The four viewports: 3D perspective + Top/Front/Side orthographic views.

mod overlays;
mod scene;
mod view2d;
mod vertex;
mod view3d;

pub use vertex::VertexState;

use crate::app::*;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

pub(crate) const SEL_COLOR: Color32 = Color32::from_rgb(255, 64, 64);

#[derive(Clone, Copy)]
pub(crate) struct Proj {
    pub(crate) rect: Rect,
    pub(crate) center: (f64, f64),
    pub(crate) zoom: f64,
}

impl Proj {
    pub(crate) fn to_screen(&self, u: f64, v: f64) -> Pos2 {
        Pos2::new(
            self.rect.center().x + ((u - self.center.0) * self.zoom) as f32,
            self.rect.center().y - ((v - self.center.1) * self.zoom) as f32,
        )
    }
    pub(crate) fn to_world(&self, p: Pos2) -> (f64, f64) {
        (
            self.center.0 + (p.x - self.rect.center().x) as f64 / self.zoom,
            self.center.1 - (p.y - self.rect.center().y) as f64 / self.zoom,
        )
    }
}

pub(crate) fn ent_color(e: &crate::formats::vmf::Entity, fgd: &crate::formats::fgd::Fgd) -> Color32 {
    if let Some(c) = e.editor_str("color") {
        if let Some(v) = crate::formats::vmf::parse_vec3(c) {
            return Color32::from_rgb(v.x as u8, v.y as u8, v.z as u8);
        }
    }
    if let Some(c) = fgd.get(e.classname()).and_then(|c| c.color) {
        return Color32::from_rgb(c[0], c[1], c[2]);
    }
    Color32::from_rgb(220, 30, 220)
}
