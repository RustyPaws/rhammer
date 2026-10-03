#![allow(dead_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod assets;
mod compile;
mod config;
mod dialogs;
mod doc;
mod fgd;
mod geom;
mod instances;
mod kv;
mod mdl;
mod model_viewer;
mod props;
mod render3d;
mod views;
mod vmf;
mod vpk;
mod vtf;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1500.0, 900.0]).with_title("rhammer"),
        depth_buffer: 24,
        multisampling: 0,
        ..Default::default()
    };
    eframe::run_native("rhammer", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
