use egui::{Align, CornerRadius, Layout, RichText, Stroke};

use crate::state::{ActivePanel, AppState, PipelineStatus};
use crate::theme::Palette;

pub fn draw_sidebar(ui: &mut egui::Ui, state: &mut AppState) {
    ui.set_min_width(220.0);
    ui.set_max_width(220.0);

    ui.add_space(8.0);

    // Logo / Brand
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new("HelioFrame")
                .size(24.0)
                .color(Palette::ACCENT)
                .strong(),
        );
        ui.label(
            RichText::new("4K Video Upscaler")
                .size(11.0)
                .color(Palette::TEXT_MUTED),
        );
    });

    ui.add_space(16.0);

    // Navigation items
    let nav_items = [
        (ActivePanel::Upscale, "\u{1F3AC}  Upscale", "Configure and launch"),
        (ActivePanel::Progress, "\u{1F4CA}  Pipeline", "Monitor progress"),
        (ActivePanel::Diagnostics, "\u{1F50D}  Diagnostics", "System health"),
        (ActivePanel::Settings, "\u{2699}  Settings", "Preferences"),
    ];

    for (panel, label, subtitle) in nav_items {
        let is_active = state.active_panel == panel;
        nav_button(ui, label, subtitle, is_active, || {
            state.active_panel = panel;
        });
    }

    ui.add_space(12.0);

    // Pipeline status summary in sidebar
    if let Ok(pipeline) = state.pipeline.lock() {
        let (status_text, status_color) = match &pipeline.status {
            PipelineStatus::Idle => ("Ready", Palette::TEXT_MUTED),
            PipelineStatus::Running => ("Processing...", Palette::ACCENT),
            PipelineStatus::Completed => ("Complete", Palette::SUCCESS),
            PipelineStatus::Failed(_) => ("Failed", Palette::ERROR),
        };

        egui::Frame::new()
            .fill(Palette::BG_DARKEST)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Status")
                        .size(11.0)
                        .color(Palette::TEXT_MUTED),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(status_text)
                        .size(13.0)
                        .color(status_color)
                        .strong(),
                );
                if pipeline.status == PipelineStatus::Running {
                    ui.add_space(4.0);
                    let progress = pipeline.overall_progress;
                    crate::widgets::progress_bar(ui, progress, Palette::ACCENT);
                    ui.label(
                        RichText::new(format!("{:.0}%", progress * 100.0))
                            .size(11.0)
                            .color(Palette::TEXT_SECONDARY),
                    );
                }
            });
    }

    // Spacer to push version info to bottom
    ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
        ui.add_space(8.0);
        ui.label(
            RichText::new("v0.1.0")
                .size(11.0)
                .color(Palette::TEXT_MUTED),
        );
        if ui
            .add(
                egui::Label::new(
                    RichText::new("About")
                        .size(11.0)
                        .color(Palette::TEXT_SECONDARY),
                )
                .sense(egui::Sense::click()),
            )
            .clicked()
        {
            state.show_about = !state.show_about;
        }
    });
}

fn nav_button(
    ui: &mut egui::Ui,
    label: &str,
    subtitle: &str,
    active: bool,
    on_click: impl FnOnce(),
) {
    let fill = if active {
        Palette::ACCENT.linear_multiply(0.12)
    } else {
        egui::Color32::TRANSPARENT
    };
    let stroke = if active {
        Stroke::new(0.0, Palette::ACCENT)
    } else {
        Stroke::NONE
    };

    let resp = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // Accent bar for active item
                if active {
                    let rect = ui.available_rect_before_wrap();
                    ui.painter().vline(
                        rect.left() - 10.0,
                        rect.y_range(),
                        Stroke::new(3.0, Palette::ACCENT),
                    );
                }
                ui.vertical(|ui| {
                    let text_color = if active {
                        Palette::ACCENT
                    } else {
                        Palette::TEXT_PRIMARY
                    };
                    ui.label(RichText::new(label).size(14.0).color(text_color).strong());
                    ui.label(
                        RichText::new(subtitle)
                            .size(11.0)
                            .color(Palette::TEXT_MUTED),
                    );
                });
            });
        })
        .response;

    if resp.interact(egui::Sense::click()).clicked() {
        on_click();
    }
}
