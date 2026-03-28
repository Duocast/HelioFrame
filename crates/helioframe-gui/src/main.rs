#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod panels;
mod state;
mod theme;
mod widgets;

use app::HelioFrameApp;
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    // Load and decode the application icon for window title bar and taskbar
    let icon_bytes = include_bytes!("../assets/icon.png");
    let icon_image =
        image::load_from_memory(icon_bytes).expect("Failed to decode embedded icon PNG");
    let icon_rgba = icon_image.to_rgba8();
    let (w, h) = icon_rgba.dimensions();
    let icon_data = egui::IconData {
        rgba: icon_rgba.into_raw(),
        width: w,
        height: h,
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("HelioFrame — 4K Video Upscaler")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(Arc::new(icon_data)),
        ..Default::default()
    };

    eframe::run_native(
        "HelioFrame",
        options,
        Box::new(|cc| Ok(Box::new(HelioFrameApp::new(cc)))),
    )
}
