#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    quicker_rs::run_native()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
