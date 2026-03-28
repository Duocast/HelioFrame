use egui::{Color32, CornerRadius, Rect, Response, Sense, Stroke, Ui, Vec2};

use crate::theme::Palette;

/// A styled card container with optional highlight border.
pub fn card_frame(ui: &mut Ui, selected: bool, add_contents: impl FnOnce(&mut Ui)) -> Response {
    let stroke = if selected {
        Stroke::new(2.0, Palette::ACCENT)
    } else {
        Stroke::new(1.0, Palette::BORDER)
    };

    let fill = if selected {
        Palette::ACCENT.linear_multiply(0.08)
    } else {
        Palette::BG_SURFACE
    };

    egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(12.0)
        .show(ui, |ui| {
            add_contents(ui);
        })
        .response
}

/// A horizontal separator with subtle styling.
pub fn subtle_separator(ui: &mut Ui) {
    ui.add_space(4.0);
    let rect = ui.available_rect_before_wrap();
    let y = rect.top();
    ui.painter().hline(
        rect.x_range(),
        y,
        Stroke::new(1.0, Palette::BORDER),
    );
    ui.add_space(6.0);
}

/// Section heading with accent underline.
pub fn section_heading(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text)
            .size(16.0)
            .color(Palette::TEXT_PRIMARY)
            .strong(),
    );
    ui.add_space(2.0);
    let rect = ui.available_rect_before_wrap();
    let y = rect.top();
    ui.painter().hline(
        rect.left()..=rect.left() + 40.0,
        y,
        Stroke::new(2.0, Palette::ACCENT),
    );
    ui.add_space(6.0);
}

/// Status badge with colored background.
pub fn status_badge(ui: &mut Ui, text: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::new(11.0, egui::FontFamily::Proportional),
        Palette::BG_DARKEST,
    );
    let desired_size = galley.size() + Vec2::new(12.0, 4.0);
    let (rect, _response) = ui.allocate_exact_size(desired_size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(4), color);
    let text_pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(text_pos, galley, Color32::PLACEHOLDER);
}

/// Accent-colored primary action button.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let button = egui::Button::new(
        egui::RichText::new(text)
            .color(Palette::BG_DARKEST)
            .strong()
            .size(14.0),
    )
    .fill(Palette::ACCENT)
    .corner_radius(CornerRadius::same(6))
    .stroke(Stroke::NONE);
    ui.add(button)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Secondary outlined button.
pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    let button = egui::Button::new(
        egui::RichText::new(text)
            .color(Palette::TEXT_PRIMARY)
            .size(14.0),
    )
    .fill(Color32::TRANSPARENT)
    .corner_radius(CornerRadius::same(6))
    .stroke(Stroke::new(1.0, Palette::BORDER_LIGHT));
    ui.add(button)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A progress bar with animated fill.
pub fn progress_bar(ui: &mut Ui, fraction: f32, color: Color32) {
    let desired_size = Vec2::new(ui.available_width(), 6.0);
    let (rect, _response) = ui.allocate_exact_size(desired_size, Sense::hover());

    // Track background
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), Palette::BG_DARKEST);

    // Fill
    if fraction > 0.0 {
        let fill_rect = Rect::from_min_size(
            rect.min,
            Vec2::new(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
        );
        ui.painter()
            .rect_filled(fill_rect, CornerRadius::same(3), color);
    }
}

/// A circular step indicator for pipeline stages.
pub fn stage_indicator(ui: &mut Ui, index: usize, status: &crate::state::StageStatus) -> Response {
    let size = 28.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let center = rect.center();
    let radius = size / 2.0 - 1.0;

    let (fill, stroke_color, text_color) = match status {
        crate::state::StageStatus::Completed => {
            (Palette::SUCCESS, Palette::SUCCESS, Palette::BG_DARKEST)
        }
        crate::state::StageStatus::Running => {
            (Palette::ACCENT, Palette::ACCENT, Palette::BG_DARKEST)
        }
        crate::state::StageStatus::Failed(_) => {
            (Palette::ERROR, Palette::ERROR, Palette::BG_DARKEST)
        }
        crate::state::StageStatus::Skipped => {
            (Palette::BG_SURFACE, Palette::TEXT_MUTED, Palette::TEXT_MUTED)
        }
        crate::state::StageStatus::Pending => {
            (Palette::BG_SURFACE, Palette::BORDER_LIGHT, Palette::TEXT_MUTED)
        }
    };

    ui.painter()
        .circle(center, radius, fill, Stroke::new(1.5, stroke_color));

    // Draw index number or checkmark
    let label = match status {
        crate::state::StageStatus::Completed => "\u{2713}".to_string(),
        _ => format!("{}", index + 1),
    };
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        &label,
        egui::FontId::new(12.0, egui::FontFamily::Proportional),
        text_color,
    );

    response
}

/// File drop zone overlay.
pub fn drop_zone_overlay(ui: &mut Ui) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(
        rect,
        CornerRadius::ZERO,
        Color32::from_black_alpha(180),
    );
    ui.painter().rect_stroke(
        rect.shrink(16.0),
        CornerRadius::same(12),
        Stroke::new(3.0, Palette::ACCENT),
        egui::StrokeKind::Outside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Drop video file here",
        egui::FontId::new(24.0, egui::FontFamily::Proportional),
        Palette::ACCENT,
    );
}
