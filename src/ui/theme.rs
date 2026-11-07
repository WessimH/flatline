//! Colours, fonts and egui style of the interface.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals,
};
use std::sync::Arc;

/// Window background.
pub const BG: Color32 = Color32::from_rgb(8, 10, 14);
/// Side and top panels.
pub const PANEL: Color32 = Color32::from_rgb(12, 15, 20);
/// Cards.
pub const CARD: Color32 = Color32::from_rgb(17, 21, 28);
/// Raised controls.
pub const CARD_HI: Color32 = Color32::from_rgb(24, 29, 38);
/// Borders.
pub const BORDER: Color32 = Color32::from_rgb(33, 40, 52);
/// Graph grid lines.
pub const GRID: Color32 = Color32::from_rgb(26, 32, 42);
/// Primary text.
pub const TEXT: Color32 = Color32::from_rgb(228, 233, 240);
/// Secondary text.
pub const DIM: Color32 = Color32::from_rgb(132, 143, 160);
/// Hints and axis labels.
pub const FAINT: Color32 = Color32::from_rgb(78, 88, 104);

/// Brand accent: corrected result, LIVE, active controls.
pub const ACCENT: Color32 = Color32::from_rgb(44, 232, 212); // result / brand
/// Correction EQ curve.
pub const VIOLET: Color32 = Color32::from_rgb(150, 120, 255); // correction
/// Headphone curve, bypass, warnings.
pub const AMBER: Color32 = Color32::from_rgb(255, 176, 64); // measured
/// Live status.
pub const GREEN: Color32 = Color32::from_rgb(88, 245, 150);
/// Errors and clipping.
pub const RED: Color32 = Color32::from_rgb(255, 80, 80);

/// `c` with opacity `a` (0 to 1).
pub fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Monospace font of `size` points.
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Proportional font of `size` points.
pub fn prop(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Installs fonts (system San Francisco when available) and the dark theme.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // Use the system San Francisco fonts when available (runtime only, not bundled).
    for (name, path, family) in [
        ("sf-pro", "/System/Library/Fonts/SFNS.ttf", FontFamily::Proportional),
        ("sf-mono", "/System/Library/Fonts/SFNSMono.ttf", FontFamily::Monospace),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(name.into(), Arc::new(FontData::from_owned(bytes)));
            fonts.families.entry(family).or_default().insert(0, name.into());
        }
    }
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, prop(11.0)),
            (TextStyle::Body, prop(13.5)),
            (TextStyle::Button, prop(13.0)),
            (TextStyle::Heading, prop(20.0)),
            (TextStyle::Monospace, mono(12.5)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.combo_width = 200.0;

        let mut v = Visuals::dark();
        v.panel_fill = PANEL;
        v.window_fill = CARD_HI;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(10);
        v.extreme_bg_color = Color32::from_rgb(10, 12, 16);
        v.faint_bg_color = CARD;
        v.override_text_color = None;
        v.selection.bg_fill = alpha(ACCENT, 0.22);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        let r = CornerRadius::same(8);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = CARD;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, DIM);
        w.noninteractive.corner_radius = r;
        w.inactive.bg_fill = CARD_HI;
        w.inactive.weak_bg_fill = CARD_HI;
        w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = r;
        w.hovered.bg_fill = Color32::from_rgb(32, 39, 51);
        w.hovered.weak_bg_fill = Color32::from_rgb(32, 39, 51);
        w.hovered.bg_stroke = Stroke::new(1.0, alpha(ACCENT, 0.5));
        w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        w.hovered.corner_radius = r;
        w.active.bg_fill = Color32::from_rgb(28, 52, 56);
        w.active.weak_bg_fill = Color32::from_rgb(28, 52, 56);
        w.active.bg_stroke = Stroke::new(1.0, ACCENT);
        w.active.fg_stroke = Stroke::new(1.0, TEXT);
        w.active.corner_radius = r;
        w.open = w.active;
        style.visuals = v;
    });
}
