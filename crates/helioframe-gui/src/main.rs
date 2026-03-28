#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod panels;
mod state;
mod theme;
mod widgets;

use app::HelioFrameApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("HelioFrame — 4K Video Upscaler")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "HelioFrame",
        options,
        Box::new(|cc| Ok(Box::new(HelioFrameApp::new(cc)))),
    )
}
