mod action;
mod app;
mod config;
mod focus;
mod search;

use app::QuickerApp;
#[cfg(not(target_arch = "wasm32"))]
use config::Config;

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::load();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([config.panel_width, config.panel_height])
            .with_min_inner_size([300.0, 200.0])
            .with_title("Quicker-RS"),
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "Quicker-RS",
        options,
        Box::new(|cc| Ok(Box::new(QuickerApp::new(cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
thread_local! {
    static WEB_RUNNER: RefCell<Option<eframe::WebRunner>> = const { RefCell::new(None) };
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    eframe::WebLogger::init(log::LevelFilter::Info).ok();

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window not available"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("document not available"))?;
    let canvas = document
        .get_element_by_id("quicker-canvas")
        .ok_or_else(|| JsValue::from_str("missing #quicker-canvas element"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;

    let runner = eframe::WebRunner::new();
    WEB_RUNNER.with(|slot| {
        slot.borrow_mut().replace(runner.clone());
    });

    wasm_bindgen_futures::spawn_local(async move {
        runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(QuickerApp::new(cc)))),
            )
            .await
            .expect("failed to start Quicker-RS web preview");
    });

    Ok(())
}
