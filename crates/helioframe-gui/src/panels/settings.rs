use egui::{CornerRadius, RichText, Stroke};

use crate::state::AppState;
use crate::theme::Palette;
use crate::widgets;

pub fn draw_settings_panel(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.add_space(4.0);
        ui.label(
            RichText::new("Settings")
                .size(22.0)
                .color(Palette::TEXT_PRIMARY)
                .strong(),
        );
        ui.label(
            RichText::new("Configure application preferences and defaults.")
                .size(13.0)
                .color(Palette::TEXT_SECONDARY),
        );
        ui.add_space(16.0);

        // ── Run Directory ───────────────────────────────────
        widgets::section_heading(ui, "Storage");

        egui::Frame::new()
            .fill(Palette::BG_SURFACE)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(12.0)
            .stroke(Stroke::new(1.0, Palette::BORDER))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("Run Directory")
                        .size(13.0)
                        .color(Palette::TEXT_PRIMARY)
                        .strong(),
                );
                ui.label(
                    RichText::new("Base directory for run artifacts and manifests.")
                        .size(11.0)
                        .color(Palette::TEXT_MUTED),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let field = egui::TextEdit::singleline(&mut state.run_directory)
                        .desired_width(ui.available_width() - 90.0)
                        .font(egui::FontId::new(13.0, egui::FontFamily::Monospace));
                    ui.add(field);
                    if widgets::secondary_button(ui, "Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            state.run_directory = path.display().to_string();
                        }
                    }
                });
            });

        ui.add_space(12.0);

        // ── Behavior ────────────────────────────────────────
        widgets::section_heading(ui, "Behavior");

        egui::Frame::new()
            .fill(Palette::BG_SURFACE)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(12.0)
            .stroke(Stroke::new(1.0, Palette::BORDER))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.auto_open_output, "")
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Auto-open output")
                                .size(13.0)
                                .color(Palette::TEXT_PRIMARY),
                        );
                        ui.label(
                            RichText::new("Open the output file in the default player after completion.")
                                .size(11.0)
                                .color(Palette::TEXT_MUTED),
                        );
                    });
                });

                ui.add_space(8.0);
                widgets::subtle_separator(ui);
                ui.add_space(4.0);

                ui.label(
                    RichText::new("Log Level")
                        .size(13.0)
                        .color(Palette::TEXT_PRIMARY)
                        .strong(),
                );
                ui.label(
                    RichText::new("Controls the verbosity of pipeline output.")
                        .size(11.0)
                        .color(Palette::TEXT_MUTED),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for level in &["error", "warn", "info", "debug", "trace"] {
                        let is_selected = state.log_level == *level;
                        let btn = egui::Button::new(
                            RichText::new(*level)
                                .size(12.0)
                                .color(if is_selected {
                                    Palette::BG_DARKEST
                                } else {
                                    Palette::TEXT_SECONDARY
                                })
                                .family(egui::FontFamily::Monospace),
                        )
                        .fill(if is_selected {
                            Palette::ACCENT
                        } else {
                            Palette::BG_DARKEST
                        })
                        .corner_radius(CornerRadius::same(4))
                        .stroke(Stroke::new(
                            1.0,
                            if is_selected {
                                Palette::ACCENT
                            } else {
                                Palette::BORDER
                            },
                        ));
                        if ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            state.log_level = level.to_string();
                        }
                    }
                });
            });

        ui.add_space(16.0);

        // ── Preset Defaults ─────────────────────────────────
        widgets::section_heading(ui, "Quick Reference");

        egui::Frame::new()
            .fill(Palette::BG_SURFACE)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(12.0)
            .stroke(Stroke::new(1.0, Palette::BORDER))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("Preset Comparison")
                        .size(14.0)
                        .color(Palette::TEXT_PRIMARY)
                        .strong(),
                );
                ui.add_space(8.0);

                egui::Grid::new("preset_reference_table")
                    .striped(true)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        // Header row
                        for header in &[
                            "Preset",
                            "Backend",
                            "Window",
                            "Tile",
                            "Steps",
                            "QC",
                        ] {
                            ui.label(
                                RichText::new(*header)
                                    .size(11.0)
                                    .color(Palette::ACCENT)
                                    .strong(),
                            );
                        }
                        ui.end_row();

                        let data = [
                            ("Preview", "fast-preview", "8", "512", "1", "No"),
                            ("Balanced", "stcdit-studio", "16", "512", "8", "Yes"),
                            ("Studio", "stcdit-studio", "20", "512", "16", "Yes"),
                            ("Experimental", "helioframe-master", "24", "512", "24", "Yes"),
                        ];

                        for row in &data {
                            for (i, cell) in [row.0, row.1, row.2, row.3, row.4, row.5]
                                .iter()
                                .enumerate()
                            {
                                let style = if i == 0 {
                                    RichText::new(*cell)
                                        .size(12.0)
                                        .color(Palette::TEXT_PRIMARY)
                                        .strong()
                                } else {
                                    RichText::new(*cell)
                                        .size(12.0)
                                        .color(Palette::TEXT_SECONDARY)
                                        .family(egui::FontFamily::Monospace)
                                };
                                ui.label(style);
                            }
                            ui.end_row();
                        }
                    });
            });

        ui.add_space(16.0);

        // ── Keyboard Shortcuts ──────────────────────────────
        widgets::section_heading(ui, "Keyboard Shortcuts");

        egui::Frame::new()
            .fill(Palette::BG_SURFACE)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(12.0)
            .stroke(Stroke::new(1.0, Palette::BORDER))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let shortcuts = [
                    ("Ctrl+O", "Open input file"),
                    ("Ctrl+Shift+S", "Save output as"),
                    ("Ctrl+Enter", "Start pipeline"),
                    ("Ctrl+1-4", "Switch panels"),
                    ("Ctrl+D", "Run diagnostics"),
                ];
                for (key, desc) in &shortcuts {
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        egui::Frame::new()
                            .fill(Palette::BG_DARKEST)
                            .corner_radius(CornerRadius::same(3))
                            .inner_margin(egui::Margin::symmetric(6, 2))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(*key)
                                        .size(11.0)
                                        .color(Palette::TEXT_PRIMARY)
                                        .family(egui::FontFamily::Monospace),
                                );
                            });
                        ui.label(
                            RichText::new(*desc)
                                .size(12.0)
                                .color(Palette::TEXT_SECONDARY),
                        );
                    });
                    ui.add_space(2.0);
                }
            });

        ui.add_space(20.0);
    });
}
