use egui::{CornerRadius, RichText, Stroke, Vec2};
use helioframe_core::{BackendKind, UpscalePreset};

use crate::state::{AppState, LogLevel, PipelineStatus};
use crate::theme::Palette;
use crate::widgets;

pub fn draw_upscale_panel(ui: &mut egui::Ui, state: &mut AppState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.set_width(ui.available_width());

        // ── Header ──────────────────────────────────────────
        ui.add_space(4.0);
        ui.label(
            RichText::new("Upscale Configuration")
                .size(22.0)
                .color(Palette::TEXT_PRIMARY)
                .strong(),
        );
        ui.label(
            RichText::new("Configure your 4K upscaling job and launch the pipeline.")
                .size(13.0)
                .color(Palette::TEXT_SECONDARY),
        );
        ui.add_space(12.0);

        // ── Input / Output Files ────────────────────────────
        widgets::section_heading(ui, "Source & Output");

        // Input file
        egui::Frame::new()
            .fill(Palette::BG_SURFACE)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(12.0)
            .stroke(Stroke::new(
                1.0,
                if state.input_path.is_empty() {
                    Palette::BORDER
                } else {
                    Palette::SUCCESS.linear_multiply(0.5)
                },
            ))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Input Video")
                            .size(12.0)
                            .color(Palette::TEXT_MUTED),
                    );
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let field = egui::TextEdit::singleline(&mut state.input_path)
                        .hint_text("Select or drop a video file...")
                        .desired_width(ui.available_width() - 90.0);
                    ui.add(field);
                    if widgets::secondary_button(ui, "Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Video", &["mp4", "mov", "mkv", "avi", "webm", "m4v"])
                            .pick_file()
                        {
                            state.input_path = path.display().to_string();
                            if state.output_path.is_empty() {
                                auto_fill_output(state);
                            }
                        }
                    }
                });
                if !state.input_path.is_empty() {
                    ui.add_space(2.0);
                    let filename = std::path::Path::new(&state.input_path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    ui.label(
                        RichText::new(format!("\u{2713} {filename}"))
                            .size(11.0)
                            .color(Palette::SUCCESS),
                    );
                }
            });

        ui.add_space(8.0);

        // Output file
        egui::Frame::new()
            .fill(Palette::BG_SURFACE)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(12.0)
            .stroke(Stroke::new(1.0, Palette::BORDER))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("Output File")
                        .size(12.0)
                        .color(Palette::TEXT_MUTED),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let field = egui::TextEdit::singleline(&mut state.output_path)
                        .hint_text("Output path (auto-generated if empty)")
                        .desired_width(ui.available_width() - 90.0);
                    ui.add(field);
                    if widgets::secondary_button(ui, "Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Video", &["mp4", "mov", "mkv"])
                            .set_file_name("output_4k.mp4")
                            .save_file()
                        {
                            state.output_path = path.display().to_string();
                        }
                    }
                });
            });

        ui.add_space(16.0);

        // ── Preset Selection ────────────────────────────────
        widgets::section_heading(ui, "Quality Preset");

        let presets = [
            (
                UpscalePreset::Preview,
                "Preview",
                "Fast 1-step preview for quick evaluation",
                "~2 min",
                Palette::INFO,
            ),
            (
                UpscalePreset::Balanced,
                "Balanced",
                "Good balance of quality and speed",
                "~8 min",
                Palette::SUCCESS,
            ),
            (
                UpscalePreset::Studio,
                "Studio",
                "Production quality with temporal QC gates",
                "~25 min",
                Palette::ACCENT,
            ),
            (
                UpscalePreset::Experimental,
                "Experimental",
                "Maximum quality, teacher-guided pipeline",
                "~60 min",
                Palette::WARNING,
            ),
        ];

        egui::Grid::new("preset_grid")
            .num_columns(2)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                for (i, (preset, name, desc, eta, color)) in presets.iter().enumerate() {
                    let selected = state.selected_preset == *preset;
                    let resp = widgets::card_frame(ui, selected, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(*name)
                                    .size(15.0)
                                    .color(if selected { *color } else { Palette::TEXT_PRIMARY })
                                    .strong(),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    RichText::new(*eta)
                                        .size(11.0)
                                        .color(Palette::TEXT_MUTED)
                                        .family(egui::FontFamily::Monospace),
                                );
                            });
                        });
                        ui.label(
                            RichText::new(*desc)
                                .size(12.0)
                                .color(Palette::TEXT_SECONDARY),
                        );
                        // Quality tier indicator
                        ui.add_space(4.0);
                        let tier = match preset {
                            UpscalePreset::Preview => 1,
                            UpscalePreset::Balanced => 2,
                            UpscalePreset::Studio => 3,
                            UpscalePreset::Experimental => 4,
                        };
                        ui.horizontal(|ui| {
                            for t in 0..4 {
                                let dot_color = if t < tier { *color } else { Palette::BG_DARKEST };
                                let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                                ui.painter().circle_filled(dot_rect.center(), 3.5, dot_color);
                            }
                            ui.label(
                                RichText::new(format!("Tier {tier}"))
                                    .size(10.0)
                                    .color(Palette::TEXT_MUTED),
                            );
                        });
                    });
                    let resp = resp.interact(egui::Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        state.selected_preset = *preset;
                        state.selected_backend = None; // reset to preset default
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });

        ui.add_space(16.0);

        // ── Backend Override ────────────────────────────────
        widgets::section_heading(ui, "Backend");

        let backends = [
            (BackendKind::ClassicalBaseline, "Classical Baseline", "Deterministic, safe fallback"),
            (BackendKind::FastPreview, "Fast Preview", "Distilled 1-step ONNX"),
            (BackendKind::SeedvrTeacher, "SeedVR Teacher", "Heavy offline teacher reference"),
            (BackendKind::StcditStudio, "StCDiT Studio", "Multi-step structural guidance"),
            (BackendKind::RealBasicVsrBridge, "RealBasicVSR", "VSR bridge backend"),
            (BackendKind::HelioFrameMaster, "HelioFrame Master", "Orchestrated flagship pipeline"),
        ];

        let default_for_preset = default_backend_for_preset(state.selected_preset);
        let current_backend = state.selected_backend.unwrap_or(default_for_preset);

        egui::Frame::new()
            .fill(Palette::BG_SURFACE)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (backend, name, desc) in &backends {
                    let is_selected = current_backend == *backend;
                    let is_default = *backend == default_for_preset;

                    let resp = ui.horizontal(|ui| {
                        // Radio-style indicator
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
                        let c = r.center();
                        ui.painter().circle_stroke(c, 7.0, Stroke::new(1.5, if is_selected { Palette::ACCENT } else { Palette::BORDER_LIGHT }));
                        if is_selected {
                            ui.painter().circle_filled(c, 4.0, Palette::ACCENT);
                        }

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(*name)
                                        .size(13.0)
                                        .color(if is_selected { Palette::TEXT_PRIMARY } else { Palette::TEXT_SECONDARY })
                                        .strong(),
                                );
                                if is_default {
                                    widgets::status_badge(ui, "DEFAULT", Palette::ACCENT_DIM);
                                }
                            });
                            ui.label(
                                RichText::new(*desc)
                                    .size(11.0)
                                    .color(Palette::TEXT_MUTED),
                            );
                        });
                    });

                    let resp = resp.response.interact(egui::Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        state.selected_backend = Some(*backend);
                    }
                    ui.add_space(2.0);
                }
            });

        ui.add_space(16.0);

        // ── Launch Controls ─────────────────────────────────
        widgets::section_heading(ui, "Launch");

        ui.horizontal(|ui| {
            ui.checkbox(&mut state.dry_run, "")
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            ui.label(
                RichText::new("Dry run")
                    .size(13.0)
                    .color(Palette::TEXT_PRIMARY),
            );
            ui.label(
                RichText::new("(plan only, no processing)")
                    .size(11.0)
                    .color(Palette::TEXT_MUTED),
            );
        });

        ui.add_space(8.0);

        let can_launch = !state.input_path.is_empty();
        let is_running = state
            .pipeline
            .lock()
            .map(|p| p.status == PipelineStatus::Running)
            .unwrap_or(false);

        ui.horizontal(|ui| {
            if !(can_launch && !is_running) { ui.disable(); }
            if widgets::primary_button(ui, if state.dry_run { "\u{25B6}  Plan Pipeline" } else { "\u{25B6}  Start Upscale" }).clicked() {
                launch_pipeline(state);
            }

            if is_running {
                ui.spinner();
                ui.label(
                    RichText::new("Processing...")
                        .size(13.0)
                        .color(Palette::ACCENT),
                );
            }
        });

        if !can_launch {
            ui.label(
                RichText::new("Select an input file to begin.")
                    .size(12.0)
                    .color(Palette::TEXT_MUTED),
            );
        }

        ui.add_space(20.0);
    });
}

fn auto_fill_output(state: &mut AppState) {
    let input = std::path::Path::new(&state.input_path);
    if let (Some(stem), Some(ext)) = (
        input.file_stem().and_then(|s| s.to_str()),
        input.extension().and_then(|e| e.to_str()),
    ) {
        let parent = input.parent().unwrap_or(std::path::Path::new("."));
        state.output_path = parent
            .join(format!("{stem}_4k.{ext}"))
            .display()
            .to_string();
    }
}

fn default_backend_for_preset(preset: UpscalePreset) -> BackendKind {
    match preset {
        UpscalePreset::Preview => BackendKind::FastPreview,
        UpscalePreset::Balanced => BackendKind::StcditStudio,
        UpscalePreset::Studio => BackendKind::StcditStudio,
        UpscalePreset::Experimental => BackendKind::HelioFrameMaster,
    }
}

fn launch_pipeline(state: &mut AppState) {
    let pipeline = state.pipeline.clone();
    let input = state.input_path.clone();
    let output = if state.output_path.is_empty() {
        auto_fill_output(state);
        state.output_path.clone()
    } else {
        state.output_path.clone()
    };
    let preset = state.selected_preset;
    let backend = state
        .selected_backend
        .unwrap_or_else(|| default_backend_for_preset(preset));
    let dry_run = state.dry_run;

    // Reset pipeline state
    if let Ok(mut p) = pipeline.lock() {
        *p = crate::state::PipelineState::default();
        p.status = PipelineStatus::Running;
        p.push_log(LogLevel::Info, format!("Starting pipeline: {input} -> {output}"));
        p.push_log(LogLevel::Info, format!("Preset: {preset}, Backend: {backend}"));
        if dry_run {
            p.push_log(LogLevel::Info, "Mode: dry run (plan only)".to_string());
        }
    }

    state.pipeline_start_time = Some(std::time::Instant::now());
    state.active_panel = crate::state::ActivePanel::Progress;

    // Run pipeline in background thread
    std::thread::spawn(move || {
        run_pipeline_thread(pipeline, input, output, preset, backend, dry_run);
    });
}

fn run_pipeline_thread(
    pipeline: std::sync::Arc<std::sync::Mutex<crate::state::PipelineState>>,
    input: String,
    output: String,
    preset: UpscalePreset,
    backend: BackendKind,
    dry_run: bool,
) {
    use helioframe_core::{AppConfig, PresetConfig, Resolution};
    use crate::state::{StageStatus, LogLevel};

    let preset_path = PresetConfig::resolve_preset_path(preset);

    let preset_cfg = match PresetConfig::load_from_file(&preset_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            if let Ok(mut p) = pipeline.lock() {
                p.status = PipelineStatus::Failed(format!("Failed to load preset: {e}"));
                p.push_log(LogLevel::Error, format!("Preset load error: {e}"));
            }
            return;
        }
    };

    let config = AppConfig {
        input,
        output,
        backend,
        preset,
        target_resolution: Resolution::UHD_4K,
    };

    if let Err(e) = config.validate() {
        if let Ok(mut p) = pipeline.lock() {
            p.status = PipelineStatus::Failed(format!("Validation error: {e}"));
            p.push_log(LogLevel::Error, format!("Config validation failed: {e}"));
        }
        return;
    }

    if dry_run {
        match helioframe_pipeline::PipelineOrchestrator::plan(&config, preset_cfg) {
            Ok(plan) => {
                if let Ok(mut p) = pipeline.lock() {
                    p.push_log(LogLevel::Info, format!("Plan complete: {} stages", plan.stages.len()));
                    p.push_log(LogLevel::Info, format!("Container: {}", plan.probe.container));
                    p.push_log(LogLevel::Info, format!("Resolution: {}", plan.probe.assumed_resolution));
                    p.push_log(LogLevel::Info, format!("Model: {}", plan.inference.summary));
                    for stage in &plan.stages {
                        p.push_log(LogLevel::Info, format!("  Stage: {} — {}", stage.name, stage.description));
                    }
                    // Mark all stages completed for dry run visualization
                    for stage in &mut p.stages {
                        stage.status = StageStatus::Completed;
                    }
                    p.overall_progress = 1.0;
                    p.status = PipelineStatus::Completed;
                    p.push_log(LogLevel::Info, "Dry run complete. No pipeline stages were executed.".to_string());
                }
            }
            Err(e) => {
                if let Ok(mut p) = pipeline.lock() {
                    p.status = PipelineStatus::Failed(format!("Plan failed: {e}"));
                    p.push_log(LogLevel::Error, format!("Planning error: {e}"));
                }
            }
        }
        return;
    }

    // Build a logger that pushes messages into the shared PipelineState.
    let log_pipeline = pipeline.clone();
    let logger = helioframe_pipeline::PipelineLogger::new(move |msg| {
        if let Ok(mut p) = log_pipeline.lock() {
            let level = match msg.level {
                helioframe_pipeline::PipelineLogLevel::Debug => LogLevel::Debug,
                helioframe_pipeline::PipelineLogLevel::Info => LogLevel::Info,
                helioframe_pipeline::PipelineLogLevel::Warn => LogLevel::Warn,
                helioframe_pipeline::PipelineLogLevel::Error => LogLevel::Error,
            };
            p.push_log(level, msg.message);
        }
    });

    // Full execution
    match helioframe_pipeline::PipelineOrchestrator::execute_with_logger(&config, preset_cfg, logger) {
        Ok(execution) => {
            if let Ok(mut p) = pipeline.lock() {
                p.run_id = Some(execution.run_layout.run_id.clone());
                p.run_dir = Some(execution.run_layout.run_dir.clone());
                for stage in &mut p.stages {
                    stage.status = StageStatus::Completed;
                }
                p.overall_progress = 1.0;
                p.status = PipelineStatus::Completed;
                p.push_log(LogLevel::Info, format!("Run complete: {}", execution.run_layout.run_id));
                p.push_log(LogLevel::Info, format!("Output: {}", execution.run_layout.run_dir.display()));
            }
        }
        Err(e) => {
            if let Ok(mut p) = pipeline.lock() {
                p.status = PipelineStatus::Failed(format!("{e}"));
                p.push_log(LogLevel::Error, format!("Pipeline error: {e}"));
            }
        }
    }
}
