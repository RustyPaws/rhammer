#![allow(dead_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod render3d;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1500.0, 900.0]).with_title("rhammer"),
        depth_buffer: 24,
        multisampling: 0,
        ..Default::default()
    };
    eframe::run_native("rhammer", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
