use egui::{CornerRadius, RichText, Stroke, Vec2};
use helioframe_core::run_doctor;

use crate::state::AppState;
use crate::theme::Palette;
use crate::widgets;

pub fn draw_diagnostics_panel(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.add_space(4.0);
        ui.label(
            RichText::new("System Diagnostics")
                .size(22.0)
                .color(Palette::TEXT_PRIMARY)
                .strong(),
        );
        ui.label(
            RichText::new("Verify that all dependencies and hardware are properly configured.")
                .size(13.0)
                .color(Palette::TEXT_SECONDARY),
        );
        ui.add_space(12.0);

        // Run Doctor button
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, "\u{1F50D}  Run Diagnostics").clicked() {
                state.doctor_summary = Some(run_doctor());
            }
            if state.doctor_summary.is_some() {
                if widgets::secondary_button(ui, "Clear").clicked() {
                    state.doctor_summary = None;
                }
            }
        });

        ui.add_space(16.0);

        if let Some(summary) = &state.doctor_summary {
            // Overall status card
            let all_pass = summary.is_ok();
            let (overall_text, overall_color, overall_icon) = if all_pass {
                ("All checks passed", Palette::SUCCESS, "\u{2713}")
            } else {
                let failed = summary.failed_checks().len();
                let text = if failed == 1 {
                    "1 check failed"
                } else {
                    "Multiple checks failed"
                };
                (text, Palette::ERROR, "\u{2717}")
            };

            egui::Frame::new()
                .fill(overall_color.linear_multiply(0.1))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(16.0)
                .stroke(Stroke::new(1.0, overall_color.linear_multiply(0.3)))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(overall_icon)
                                .size(28.0)
                                .color(overall_color),
                        );
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(overall_text)
                                    .size(16.0)
                                    .color(overall_color)
                                    .strong(),
                            );
                            ui.label(
                                RichText::new(format!("Platform: {}", summary.platform_notice))
                                    .size(12.0)
                                    .color(Palette::TEXT_SECONDARY),
                            );
                        });
                    });
                });

            ui.add_space(16.0);

            // Individual check cards
            widgets::section_heading(ui, "Dependency Checks");

            for check in &summary.checks {
                let (icon, color) = if check.passed {
                    ("\u{2713}", Palette::SUCCESS)
                } else {
                    ("\u{2717}", Palette::ERROR)
                };

                egui::Frame::new()
                    .fill(Palette::BG_SURFACE)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(12.0)
                    .stroke(Stroke::new(
                        1.0,
                        if check.passed {
                            Palette::BORDER
                        } else {
                            Palette::ERROR.linear_multiply(0.3)
                        },
                    ))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            // Status icon
                            let (icon_rect, _) =
                                ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::hover());
                            ui.painter().circle_filled(
                                icon_rect.center(),
                                11.0,
                                color.linear_multiply(0.15),
                            );
                            ui.painter().text(
                                icon_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                icon,
                                egui::FontId::new(14.0, egui::FontFamily::Proportional),
                                color,
                            );

                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(check.name)
                                        .size(14.0)
                                        .color(Palette::TEXT_PRIMARY)
                                        .strong(),
                                );
                                ui.label(
                                    RichText::new(&check.detail)
                                        .size(12.0)
                                        .color(Palette::TEXT_SECONDARY)
                                        .family(egui::FontFamily::Monospace),
                                );
                                if let Some(action) = &check.action {
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("Fix:")
                                                .size(11.0)
                                                .color(Palette::WARNING)
                                                .strong(),
                                        );
                                        ui.label(
                                            RichText::new(action)
                                                .size(11.0)
                                                .color(Palette::TEXT_SECONDARY),
                                        );
                                    });
                                }
                            });

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if check.passed {
                                        widgets::status_badge(ui, "PASS", Palette::SUCCESS);
                                    } else {
                                        widgets::status_badge(ui, "FAIL", Palette::ERROR);
                                    }
                                },
                            );
                        });
                    });

                ui.add_space(6.0);
            }

            ui.add_space(16.0);

            // System info summary
            widgets::section_heading(ui, "Environment");

            egui::Frame::new()
                .fill(Palette::BG_SURFACE)
                .corner_radius(CornerRadius::same(8))
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let info_rows = [
                        ("OS", std::env::consts::OS),
                        ("Architecture", std::env::consts::ARCH),
                        ("Target", "Linux/NVIDIA/SDR"),
                    ];
                    for (label, value) in &info_rows {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{label}:"))
                                    .size(12.0)
                                    .color(Palette::TEXT_MUTED),
                            );
                            ui.label(
                                RichText::new(*value)
                                    .size(12.0)
                                    .color(Palette::TEXT_PRIMARY)
                                    .family(egui::FontFamily::Monospace),
                            );
                        });
                    }
                });
        } else {
            // Empty state
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{1F50D}")
                        .size(48.0)
                        .color(Palette::TEXT_MUTED),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Run diagnostics to check system readiness")
                        .size(15.0)
                        .color(Palette::TEXT_MUTED),
                );
                ui.label(
                    RichText::new("Checks for FFmpeg, Python, NVIDIA GPU, and writable directories.")
                        .size(12.0)
                        .color(Palette::TEXT_MUTED),
                );
            });
        }

        ui.add_space(20.0);
    });
}
