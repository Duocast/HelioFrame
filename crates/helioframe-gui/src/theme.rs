use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, Style, TextStyle, Visuals};

/// HelioFrame brand palette — warm solar tones on a dark base.
pub struct Palette;

impl Palette {
    // Backgrounds
    pub const BG_DARKEST: Color32 = Color32::from_rgb(14, 16, 20);
    pub const BG_DARK: Color32 = Color32::from_rgb(20, 22, 28);
    pub const BG_PANEL: Color32 = Color32::from_rgb(26, 29, 36);
    pub const BG_SURFACE: Color32 = Color32::from_rgb(34, 38, 48);
    pub const BG_HOVER: Color32 = Color32::from_rgb(42, 47, 58);

    // Accent — warm amber/gold
    pub const ACCENT: Color32 = Color32::from_rgb(255, 183, 77);
    pub const ACCENT_DIM: Color32 = Color32::from_rgb(200, 140, 50);
    #[allow(dead_code)]
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(255, 200, 110);

    // Status
    pub const SUCCESS: Color32 = Color32::from_rgb(102, 204, 153);
    pub const WARNING: Color32 = Color32::from_rgb(255, 183, 77);
    pub const ERROR: Color32 = Color32::from_rgb(239, 108, 108);
    pub const INFO: Color32 = Color32::from_rgb(100, 180, 255);

    // Text
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(230, 232, 240);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(160, 165, 180);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(100, 106, 124);

    // Borders
    pub const BORDER: Color32 = Color32::from_rgb(50, 55, 68);
    pub const BORDER_LIGHT: Color32 = Color32::from_rgb(60, 66, 80);
}

pub fn apply_theme(ctx: &egui::Context) {
    let mut style = Style::default();

    // Typography
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(22.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(14.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(12.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(14.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(13.0, FontFamily::Monospace),
    );

    // Spacing
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(16);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);
    style.spacing.indent = 20.0;

    // Visuals
    let mut visuals = Visuals::dark();
    visuals.panel_fill = Palette::BG_PANEL;
    visuals.window_fill = Palette::BG_DARK;
    visuals.extreme_bg_color = Palette::BG_DARKEST;
    visuals.faint_bg_color = Palette::BG_SURFACE;

    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.window_stroke = Stroke::new(1.0, Palette::BORDER);

    // Widget visuals
    visuals.widgets.noninteractive.bg_fill = Palette::BG_SURFACE;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Palette::TEXT_SECONDARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Palette::BORDER);

    visuals.widgets.inactive.bg_fill = Palette::BG_SURFACE;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Palette::TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Palette::BORDER);

    visuals.widgets.hovered.bg_fill = Palette::BG_HOVER;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Palette::TEXT_PRIMARY);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Palette::ACCENT_DIM);

    visuals.widgets.active.bg_fill = Palette::ACCENT_DIM;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Palette::BG_DARKEST);
    visuals.widgets.active.corner_radius = CornerRadius::same(6);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, Palette::ACCENT);

    visuals.selection.bg_fill = Palette::ACCENT.linear_multiply(0.2);
    visuals.selection.stroke = Stroke::new(1.0, Palette::ACCENT);

    visuals.hyperlink_color = Palette::ACCENT;
    visuals.warn_fg_color = Palette::WARNING;
    visuals.error_fg_color = Palette::ERROR;

    style.visuals = visuals;
    ctx.set_style(style);
}
