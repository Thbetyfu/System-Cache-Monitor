//! Cache-Advisor — agentic storage advisor built in Rust + egui.
//!
//! Three panels:
//! 1. **Scan** — table of monitored folders, color-coded by risk, with clean buttons.
//! 2. **Archive** — advisory list of folders to move to external storage.
//! 3. **Ask AI** — on-demand LLM Q&A (feature-gated, only with `--features ai`).

mod ui;
mod tray;

use eframe::egui;

fn main() -> eframe::Result<()> {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 700.0])
            .with_title("Cache Advisor v0.2"),
        ..Default::default()
    };

    eframe::run_native(
        "Cache Advisor",
        options,
        Box::new(|cc| {
            // Enable custom egui styles (dark mode).
            setup_style(&cc.egui_ctx);
            Ok(Box::new(ui::App::new()))
        }),
    )
}

fn setup_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    ctx.set_style(style);
}
