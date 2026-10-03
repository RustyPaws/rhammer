//! The four viewports: 3D perspective + Top/Front/Side orthographic views.

mod overlays;
mod scene;
mod view2d;
mod view3d;

use crate::app::*;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

pub(crate) const SEL_COLOR: Color32 = Color32::from_rgb(255, 64, 64);

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

impl App {
    pub fn views_ui(&mut self, ui: &mut egui::Ui) {
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        let gap = 3.0;
        let rects: [Rect; 4] = if let Some(m) = self.maximized {
            let mut r = [Rect::NOTHING; 4];
            r[m] = full;
            r
        } else {
            let w = (full.width() - gap) / 2.0;
            let h = (full.height() - gap) / 2.0;
            let tl = Rect::from_min_size(full.min, Vec2::new(w, h));
            let tr = Rect::from_min_size(Pos2::new(full.min.x + w + gap, full.min.y), Vec2::new(w, h));
            let bl = Rect::from_min_size(Pos2::new(full.min.x, full.min.y + h + gap), Vec2::new(w, h));
            let br = Rect::from_min_size(Pos2::new(full.min.x + w + gap, full.min.y + h + gap), Vec2::new(w, h));
            [tl, tr, bl, br]
        };
        self.hover_world = None;
        self.prepare_scene();
        for i in 0..4 {
            if rects[i] == Rect::NOTHING {
                continue;
            }
            if i == 0 {
                self.view3d_ui(ui, rects[0]);
            } else {
                self.view2d_ui(ui, rects[i], i - 1);
            }
            // title
            let p = ui.painter_at(rects[i]);
            p.text(
                rects[i].min + Vec2::new(6.0, 4.0),
                egui::Align2::LEFT_TOP,
                VIEW_NAMES[i],
                egui::FontId::proportional(12.0),
                Color32::from_gray(200),
            );
            p.rect_stroke(rects[i], 0.0, Stroke::new(1.0, Color32::from_gray(70)), egui::StrokeKind::Inside);
        }
    }
}

pub(crate) fn view_zoom(v: &View2D) -> f64 {
    v.zoom
}
