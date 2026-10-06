mod action;
#[cfg(target_os = "linux")]
mod activation;
mod app;
#[cfg(target_os = "linux")]
mod clipboard_monitor;
mod config;
mod focus;
#[cfg(target_os = "linux")]
mod input_windows;
#[cfg(not(target_arch = "wasm32"))]
mod process;
mod search;
#[cfg(not(target_arch = "wasm32"))]
mod storage;
#[cfg(not(target_arch = "wasm32"))]
mod text_windows;
#[cfg(not(target_arch = "wasm32"))]
mod wait_windows;
#[cfg(target_os = "linux")]
mod x11;

use app::QuickerApp;
#[cfg(not(target_arch = "wasm32"))]
use config::Config;

/// Check an exported action without opening a window or executing its steps.
/// Returns a JSON report and a CI exit code (0: no known gaps, 1: gaps, 2: invalid).
pub fn check_plugin_json(input: &str) -> (serde_json::Value, i32) {
    let report = action::compatibility::inspect(input);
    let code = action::compatibility::exit_code(&report);
    (report, code)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn check_plugin_file(path: &std::path::Path) -> (serde_json::Value, i32) {
    use std::io::Read;
    const LIMIT: u64 = 16 * 1024 * 1024;
    let result = (|| {
        let mut input = String::new();
        std::fs::File::open(path)?
            .take(LIMIT + 1)
            .read_to_string(&mut input)?;
        if input.len() as u64 > LIMIT {
            return Err(std::io::Error::other("Plugin exceeds 16 MiB limit"));
        }
        Ok(input)
    })();
    match result {
        Ok(input) => check_plugin_json(&input),
        Err(error) => (
            serde_json::json!({"schema_version": 1, "import": {"status": "error", "error": error.to_string()}}),
            2,
        ),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Quicker-RS\nUsage: quicker-rs [--show | --toggle | --hide | --hidden | --quit | --check-config]\n       quicker-rs --check-plugin FILE\n\n--check-plugin prints a JSON compatibility report without executing the action.\nExit codes: 0 = no known static gaps, 1 = gaps, 2 = invalid input.\nRun again to show the existing panel. On Wayland, bind quicker-rs --toggle\nto a shortcut in your desktop settings. --hidden starts in the background.");
        return Ok(());
    }
    if args.len() > 1
        || args.first().is_some_and(|arg| {
            ![
                "--show",
                "--toggle",
                "--hide",
                "--hidden",
                "--quit",
                "--check-config",
            ]
            .contains(&arg.as_str())
        })
    {
        return Err(eframe::Error::AppCreation(
            std::io::Error::other("Unknown arguments; use --help").into(),
        ));
    }
    let hidden = args
        .first()
        .is_some_and(|arg| arg == "--hidden" || arg == "--hide");
    #[cfg(target_os = "linux")]
    let activation = if args.first().is_some_and(|arg| arg == "--check-config") {
        None
    } else {
        use activation::{Instance, Request};
        let request = match args.first().map(String::as_str) {
            Some("--toggle") => Request::Toggle,
            Some("--hide" | "--hidden") => Request::Hide,
            Some("--quit") => Request::Quit,
            _ => Request::Show,
        };
        let dir =
            activation::runtime_dir().map_err(|err| eframe::Error::AppCreation(err.into()))?;
        match activation::start(&dir, request)
            .map_err(|err| eframe::Error::AppCreation(err.into()))?
        {
            Instance::Forwarded => return Ok(()),
            Instance::Primary(_) if request == Request::Quit => return Ok(()),
            Instance::Primary(server) => Some(server),
        }
    };
    let config = Config::load()
        .map_err(|err| eframe::Error::AppCreation(std::io::Error::other(err).into()))?;
    if args.first().is_some_and(|arg| arg == "--check-config") {
        println!(
            "Valid config: {} ({} profiles)",
            Config::config_path().display(),
            config.profiles.len()
        );
        return Ok(());
    }
    // Capture before eframe creates and focuses the launcher's window.
    #[cfg(target_os = "linux")]
    if let Err(error) = clipboard_monitor::snapshot() {
        log::debug!("Clipboard monitoring is unavailable: {error}");
    }
    let initial_focus = focus::detect_focused_process();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([config.panel_width, config.panel_height])
            .with_min_inner_size([300.0, 200.0])
            .with_title("Quicker-RS")
            .with_app_id("net.getquicker.QuickerRS")
            .with_visible(!hidden),
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "Quicker-RS",
        options,
        Box::new(move |cc| {
            let mut app = QuickerApp::new(cc, config);
            app.set_initial_focus(initial_focus);
            app.set_initial_visibility(hidden);
            #[cfg(target_os = "linux")]
            app.set_activation(activation);
            Ok(Box::new(app))
        }),
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
                Box::new(|cc| Ok(Box::new(QuickerApp::new(cc, config::Config::default())))),
            )
            .await
            .expect("failed to start Quicker-RS web preview");
    });

    Ok(())
}
