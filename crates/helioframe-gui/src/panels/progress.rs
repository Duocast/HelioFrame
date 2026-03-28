use egui::{CornerRadius, RichText, Stroke, Vec2};

use crate::state::{AppState, LogLevel, PipelineStatus, StageStatus};
use crate::theme::Palette;
use crate::widgets;

pub fn draw_progress_panel(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.add_space(4.0);
        ui.label(
            RichText::new("Pipeline Progress")
                .size(22.0)
                .color(Palette::TEXT_PRIMARY)
                .strong(),
        );
        ui.label(
            RichText::new("Real-time view of the 12-stage upscaling pipeline.")
                .size(13.0)
                .color(Palette::TEXT_SECONDARY),
        );
        ui.add_space(12.0);

        let pipeline = state.pipeline.lock().unwrap().clone();

        // ── Overall Progress Bar ────────────────────────────
        egui::Frame::new()
            .fill(Palette::BG_SURFACE)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (status_text, status_color) = match &pipeline.status {
                        PipelineStatus::Idle => ("Idle — no job running", Palette::TEXT_MUTED),
                        PipelineStatus::Running => ("Processing", Palette::ACCENT),
                        PipelineStatus::Completed => ("Completed successfully", Palette::SUCCESS),
                        PipelineStatus::Failed(_) => ("Failed", Palette::ERROR),
                    };
                    ui.label(
                        RichText::new(status_text)
                            .size(16.0)
                            .color(status_color)
                            .strong(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(start) = state.pipeline_start_time {
                            let elapsed = start.elapsed();
                            ui.label(
                                RichText::new(format_duration(elapsed))
                                    .size(14.0)
                                    .color(Palette::TEXT_SECONDARY)
                                    .family(egui::FontFamily::Monospace),
                            );
                        }
                    });
                });

                if let PipelineStatus::Failed(msg) = &pipeline.status {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(msg)
                            .size(12.0)
                            .color(Palette::ERROR),
                    );
                }

                ui.add_space(8.0);
                widgets::progress_bar(ui, pipeline.overall_progress, match &pipeline.status {
                    PipelineStatus::Completed => Palette::SUCCESS,
                    PipelineStatus::Failed(_) => Palette::ERROR,
                    _ => Palette::ACCENT,
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let completed = pipeline.stages.iter().filter(|s| s.status == StageStatus::Completed).count();
                    ui.label(
                        RichText::new(format!("{completed} / {} stages", pipeline.stages.len()))
                            .size(12.0)
                            .color(Palette::TEXT_MUTED),
                    );
                });

                // Run info
                if let Some(run_id) = &pipeline.run_id {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Run ID:")
                                .size(11.0)
                                .color(Palette::TEXT_MUTED),
                        );
                        ui.label(
                            RichText::new(run_id)
                                .size(11.0)
                                .color(Palette::TEXT_SECONDARY)
                                .family(egui::FontFamily::Monospace),
                        );
                    });
                }
            });

        ui.add_space(16.0);

        // ── Stage Timeline ──────────────────────────────────
        widgets::section_heading(ui, "Stage Timeline");

        egui::Frame::new()
            .fill(Palette::BG_SURFACE)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // Horizontal stage indicators
                ui.horizontal_wrapped(|ui| {
                    for (i, stage) in pipeline.stages.iter().enumerate() {
                        widgets::stage_indicator(ui, i, &stage.status);
                        if i < pipeline.stages.len() - 1 {
                            // Connector line
                            let (line_rect, _) =
                                ui.allocate_exact_size(Vec2::new(16.0, 28.0), egui::Sense::hover());
                            let y = line_rect.center().y;
                            let color = if stage.status == StageStatus::Completed {
                                Palette::SUCCESS.linear_multiply(0.6)
                            } else {
                                Palette::BORDER
                            };
                            ui.painter().hline(
                                line_rect.x_range(),
                                y,
                                Stroke::new(2.0, color),
                            );
                        }
                    }
                });

                ui.add_space(12.0);
                widgets::subtle_separator(ui);
                ui.add_space(4.0);

                // Detailed stage list
                for (i, stage) in pipeline.stages.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (status_icon, color) = match &stage.status {
                            StageStatus::Completed => ("\u{2713}", Palette::SUCCESS),
                            StageStatus::Running => ("\u{25B6}", Palette::ACCENT),
                            StageStatus::Failed(_) => ("\u{2717}", Palette::ERROR),
                            StageStatus::Skipped => ("\u{2014}", Palette::TEXT_MUTED),
                            StageStatus::Pending => ("\u{25CB}", Palette::TEXT_MUTED),
                        };

                        ui.label(
                            RichText::new(format!("{:>2}.", i + 1))
                                .size(12.0)
                                .color(Palette::TEXT_MUTED)
                                .family(egui::FontFamily::Monospace),
                        );
                        ui.label(RichText::new(status_icon).size(14.0).color(color));
                        ui.label(
                            RichText::new(stage.name)
                                .size(13.0)
                                .color(color)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(stage.description)
                                .size(12.0)
                                .color(Palette::TEXT_SECONDARY),
                        );

                        if let StageStatus::Running = &stage.status {
                            ui.spinner();
                        }
                    });

                    if let StageStatus::Failed(msg) = &stage.status {
                        ui.indent(format!("stage_err_{i}"), |ui| {
                            ui.label(
                                RichText::new(msg)
                                    .size(11.0)
                                    .color(Palette::ERROR),
                            );
                        });
                    }
                }
            });

        ui.add_space(16.0);

        // ── Log Console ─────────────────────────────────────
        widgets::section_heading(ui, "Log Output");

        egui::Frame::new()
            .fill(Palette::BG_DARKEST)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(12.0)
            .stroke(Stroke::new(1.0, Palette::BORDER))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(200.0);

                if pipeline.log_lines.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.label(
                            RichText::new("No log output yet. Start a pipeline to see logs here.")
                                .size(13.0)
                                .color(Palette::TEXT_MUTED),
                        );
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for entry in &pipeline.log_lines {
                                let color = match entry.level {
                                    LogLevel::Info => Palette::TEXT_SECONDARY,
                                    LogLevel::Warn => Palette::WARNING,
                                    LogLevel::Error => Palette::ERROR,
                                    LogLevel::Debug => Palette::TEXT_MUTED,
                                };
                                let level_str = match entry.level {
                                    LogLevel::Info => "INFO ",
                                    LogLevel::Warn => "WARN ",
                                    LogLevel::Error => "ERROR",
                                    LogLevel::Debug => "DEBUG",
                                };
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&entry.timestamp)
                                            .size(11.0)
                                            .color(Palette::TEXT_MUTED)
                                            .family(egui::FontFamily::Monospace),
                                    );
                                    ui.label(
                                        RichText::new(level_str)
                                            .size(11.0)
                                            .color(color)
                                            .family(egui::FontFamily::Monospace),
                                    );
                                    ui.label(
                                        RichText::new(&entry.message)
                                            .size(12.0)
                                            .color(Palette::TEXT_PRIMARY)
                                            .family(egui::FontFamily::Monospace),
                                    );
                                });
                            }
                        });
                }
            });

        ui.add_space(20.0);

        // Request repaints while running
        if pipeline.status == PipelineStatus::Running {
            ui.ctx().request_repaint();
        }
    });
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m {:02}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}
