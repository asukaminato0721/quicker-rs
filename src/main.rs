#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--check-plugin") {
        let (report, code) = if args.len() == 2 {
            quicker_rs::check_plugin_file(std::path::Path::new(&args[1]))
        } else {
            (
                serde_json::json!({"schema_version": 1, "error": "Usage: quicker-rs --check-plugin FILE"}),
                2,
            )
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("JSON report")
        );
        std::process::exit(code);
    }
    quicker_rs::run_native()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
