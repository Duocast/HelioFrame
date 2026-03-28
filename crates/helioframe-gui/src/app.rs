use egui::{Panel, Stroke, Ui};

use crate::panels::{about, diagnostics, progress, settings, sidebar, upscale};
use crate::state::{ActivePanel, AppState};
use crate::theme::{self, Palette};

pub struct HelioFrameApp {
    state: AppState,
}

impl HelioFrameApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply_theme(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Self {
            state: AppState::default(),
        }
    }
}

impl eframe::App for HelioFrameApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Handle keyboard shortcuts
        handle_shortcuts(&ctx, &mut self.state);

        // Handle file drops
        handle_file_drops(&ctx, &mut self.state);

        // Drop zone overlay
        if self.state.file_drop_hover {
            egui::Area::new(egui::Id::new("drop_overlay"))
                .fixed_pos(egui::Pos2::ZERO)
                .order(egui::Order::Foreground)
                .show(&ctx, |ui| {
                    crate::widgets::drop_zone_overlay(ui);
                });
        }

        // ── Sidebar ─────────────────────────────────────────
        Panel::left("sidebar")
            .exact_size(220.0)
            .frame(
                egui::Frame::new()
                    .fill(Palette::BG_DARK)
                    .stroke(Stroke::new(1.0, Palette::BORDER))
                    .inner_margin(egui::Margin::symmetric(8, 12)),
            )
            .show_inside(ui, |ui| {
                sidebar::draw_sidebar(ui, &mut self.state);
            });

        // ── Main Content (rendered in the remaining central area) ──
        egui::Frame::new()
            .fill(Palette::BG_PANEL)
            .inner_margin(egui::Margin::symmetric(24, 16))
            .show(ui, |ui| {
                match self.state.active_panel {
                    ActivePanel::Upscale => upscale::draw_upscale_panel(ui, &mut self.state),
                    ActivePanel::Progress => progress::draw_progress_panel(ui, &mut self.state),
                    ActivePanel::Diagnostics => {
                        diagnostics::draw_diagnostics_panel(ui, &mut self.state)
                    }
                    ActivePanel::Settings => settings::draw_settings_panel(ui, &mut self.state),
                    ActivePanel::About => about::draw_about_panel(ui, &mut self.state),
                }
            });
    }
}

fn handle_shortcuts(ctx: &egui::Context, state: &mut AppState) {
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

fn handle_file_drops(ctx: &egui::Context, state: &mut AppState) {
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
