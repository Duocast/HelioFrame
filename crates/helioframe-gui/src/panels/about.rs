use egui::{CornerRadius, RichText, Stroke};

use crate::state::AppState;
use crate::theme::Palette;
use crate::widgets;

pub fn draw_about_panel(ui: &mut egui::Ui, _state: &mut AppState) {
    ui.add_space(8.0);
    widgets::section_heading(ui, "About HelioFrame");
    ui.add_space(16.0);

    egui::Frame::new()
        .fill(Palette::BG_DARK)
        .stroke(Stroke::new(1.0, Palette::BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(32.0)
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                // Logo
                let logo_bytes = include_bytes!("../../assets/logo.png");
                ui.add(
                    egui::Image::from_bytes("bytes://helioframe_logo.png", logo_bytes.as_slice())
                        .max_size(egui::vec2(180.0, 180.0))
                        .corner_radius(CornerRadius::same(12)),
                );

                ui.add_space(16.0);

                // Title
                ui.label(
                    RichText::new("HelioFrame")
                        .size(32.0)
                        .color(Palette::ACCENT)
                        .strong(),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Quality-First 4K Video Upscaler")
                        .size(16.0)
                        .color(Palette::TEXT_SECONDARY),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("v0.1.0")
                        .size(13.0)
                        .color(Palette::TEXT_MUTED)
                        .family(egui::FontFamily::Monospace),
                );

                ui.add_space(24.0);

                // Description
                ui.label(
                    RichText::new(
                        "HelioFrame is an intelligent video upscaling application that \
                         transforms your content to 3840\u{00D7}2160 (4K) resolution. \
                         Built with a quality-first philosophy, it prioritizes perceptual \
                         richness, temporal stability, and structural fidelity over raw speed.",
                    )
                    .size(13.0)
                    .color(Palette::TEXT_SECONDARY),
                );

                ui.add_space(20.0);

                // Tech stack
                egui::Frame::new()
                    .fill(Palette::BG_DARKEST)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Technology Stack")
                                .size(12.0)
                                .color(Palette::ACCENT)
                                .strong(),
                        );
                        ui.add_space(8.0);
                        for item in [
                            "Rust \u{2014} Systems-level performance and safety",
                            "ONNX Runtime \u{2014} Cross-platform model inference",
                            "FFmpeg \u{2014} Industry-standard video encoding/decoding",
                            "egui \u{2014} Immediate-mode GPU-accelerated GUI",
                        ] {
                            ui.label(
                                RichText::new(format!("  \u{2022}  {item}"))
                                    .size(12.0)
                                    .color(Palette::TEXT_SECONDARY),
                            );
                        }
                    });

                ui.add_space(16.0);

                ui.label(
                    RichText::new("Licensed under MIT")
                        .size(11.0)
                        .color(Palette::TEXT_MUTED),
                );
            });
        });
}
