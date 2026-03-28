use eframe::Frame;
use egui::{CentralPanel, Context, CornerRadius, RichText, SidePanel, Stroke};

use crate::panels::{diagnostics, progress, settings, sidebar, upscale};
use crate::state::{ActivePanel, AppState};
use crate::theme::{self, Palette};

pub struct HelioFrameApp {
    state: AppState,
}

impl HelioFrameApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply_theme(&cc.egui_ctx);
        Self {
            state: AppState::default(),
        }
    }
}

impl eframe::App for HelioFrameApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        // Handle keyboard shortcuts
        handle_shortcuts(ctx, &mut self.state);

        // Handle file drops
        handle_file_drops(ctx, &mut self.state);

        // About dialog
        if self.state.show_about {
            draw_about_window(ctx, &mut self.state);
        }

        // Drop zone overlay
        if self.state.file_drop_hover {
            egui::Area::new(egui::Id::new("drop_overlay"))
                .fixed_pos(egui::Pos2::ZERO)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    crate::widgets::drop_zone_overlay(ui);
                });
        }

        // ── Sidebar ─────────────────────────────────────────
        SidePanel::left("sidebar")
            .exact_width(220.0)
            .frame(
                egui::Frame::new()
                    .fill(Palette::BG_DARK)
                    .stroke(Stroke::new(1.0, Palette::BORDER))
                    .inner_margin(egui::Margin::symmetric(8, 12)),
            )
            .show(ctx, |ui| {
                sidebar::draw_sidebar(ui, &mut self.state);
            });

        // ── Main Content ────────────────────────────────────
        CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Palette::BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ctx, |ui| {
                match self.state.active_panel {
                    ActivePanel::Upscale => upscale::draw_upscale_panel(ui, &mut self.state),
                    ActivePanel::Progress => progress::draw_progress_panel(ui, &mut self.state),
                    ActivePanel::Diagnostics => {
                        diagnostics::draw_diagnostics_panel(ui, &mut self.state)
                    }
                    ActivePanel::Settings => settings::draw_settings_panel(ui, &mut self.state),
                }
            });
    }
}

fn handle_shortcuts(ctx: &Context, state: &mut AppState) {
    ctx.input(|i| {
        // Ctrl+1-4 for panel switching
        if i.modifiers.command {
            if i.key_pressed(egui::Key::Num1) {
                state.active_panel = ActivePanel::Upscale;
            } else if i.key_pressed(egui::Key::Num2) {
                state.active_panel = ActivePanel::Progress;
            } else if i.key_pressed(egui::Key::Num3) {
                state.active_panel = ActivePanel::Diagnostics;
            } else if i.key_pressed(egui::Key::Num4) {
                state.active_panel = ActivePanel::Settings;
            }

            // Ctrl+O for open file
            if i.key_pressed(egui::Key::O) {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Video", &["mp4", "mov", "mkv", "avi", "webm", "m4v"])
                    .pick_file()
                {
                    state.input_path = path.display().to_string();
                    state.active_panel = ActivePanel::Upscale;
                }
            }

            // Ctrl+D for diagnostics
            if i.key_pressed(egui::Key::D) {
                state.active_panel = ActivePanel::Diagnostics;
                state.doctor_summary = Some(helioframe_core::run_doctor());
            }
        }
    });
}

fn handle_file_drops(ctx: &Context, state: &mut AppState) {
    // Track hover state for overlay
    ctx.input(|i| {
        state.file_drop_hover = !i.raw.hovered_files.is_empty();
    });

    // Handle actual drops
    let dropped: Vec<_> = ctx.input(|i| i.raw.dropped_files.clone());
    if let Some(file) = dropped.first() {
        if let Some(path) = &file.path {
            state.input_path = path.display().to_string();
            state.active_panel = ActivePanel::Upscale;
            // Auto-fill output
            if let (Some(stem), Some(ext)) = (
                path.file_stem().and_then(|s| s.to_str()),
                path.extension().and_then(|e| e.to_str()),
            ) {
                let parent = path.parent().unwrap_or(std::path::Path::new("."));
                state.output_path = parent
                    .join(format!("{stem}_4k.{ext}"))
                    .display()
                    .to_string();
            }
        }
    }
}

fn draw_about_window(ctx: &Context, state: &mut AppState) {
    egui::Window::new("About HelioFrame")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([400.0, 280.0])
        .frame(
            egui::Frame::new()
                .fill(Palette::BG_DARK)
                .stroke(Stroke::new(1.0, Palette::BORDER))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(24.0),
        )
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("HelioFrame")
                        .size(28.0)
                        .color(Palette::ACCENT)
                        .strong(),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Quality-First 4K Video Upscaler")
                        .size(14.0)
                        .color(Palette::TEXT_SECONDARY),
                );
                ui.add_space(12.0);
                ui.label(
                    RichText::new("v0.1.0")
                        .size(13.0)
                        .color(Palette::TEXT_MUTED)
                        .family(egui::FontFamily::Monospace),
                );
                ui.add_space(16.0);
                ui.label(
                    RichText::new(
                        "Intelligent video upscaling to 3840\u{00D7}2160 with \
                         perceptual richness, temporal stability, and structural fidelity.",
                    )
                    .size(12.0)
                    .color(Palette::TEXT_SECONDARY),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Rust + ONNX Runtime + FFmpeg")
                        .size(11.0)
                        .color(Palette::TEXT_MUTED)
                        .family(egui::FontFamily::Monospace),
                );
                ui.add_space(16.0);
                if ui
                    .button(RichText::new("Close").color(Palette::TEXT_PRIMARY))
                    .clicked()
                {
                    state.show_about = false;
                }
            });
        });
}
