#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rhammer::app::App;

#[cfg(feature = "local")]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1500.0, 900.0]).with_title("rhammer"),
        depth_buffer: 24,
        multisampling: 0,
        ..Default::default()
    };
    eframe::run_native("rhammer", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

#[cfg(feature = "web")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    console_error_panic_hook::set_once();
    let options = eframe::WebOptions { depth_buffer: 24, ..Default::default() };
    wasm_bindgen_futures::spawn_local(async move {
        let document = web_sys::window().expect("no window").document().expect("no document");
        let canvas = document
            .get_element_by_id("rhammer_canvas")
            .expect("missing #rhammer_canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#rhammer_canvas is not a canvas");
        let started = eframe::WebRunner::new().start(canvas, options, Box::new(|cc| Ok(Box::new(App::new(cc))))).await;
        // remove the loading text, or show why startup failed
        if let Some(loading) = document.get_element_by_id("loading") {
            match started {
                Ok(()) => loading.remove(),
                Err(e) => {
                    loading.set_inner_html("rhammer failed to start (WebGL2 is required). See the console.");
                    panic!("failed to start eframe: {e:?}");
                }
            }
        }
    });
}
