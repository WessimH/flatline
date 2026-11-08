//! "Horizon" art direction: colours, fonts and egui style.
//!
//! Night background, a living light (pulse red, halo coral, sun gold) behind
//! frosted-glass cards, milk-white text. Titles use Syne ExtraBold, the
//! interface and the huge thin numbers use Outfit (both SIL OFL, embedded as
//! variable fonts and instanced per weight).

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, FontTweak, Stroke, TextStyle, Visuals,
};
use eframe::epaint::text::VariationCoords;
use std::sync::Arc;

/// Window background.
pub const NIGHT: Color32 = Color32::from_rgb(7, 6, 10);
/// Primary text, white controls.
pub const MILK: Color32 = Color32::from_rgb(244, 239, 234);
/// Secondary text.
pub const MIST: Color32 = Color32::from_rgb(217, 207, 203);
/// Tertiary text, captions.
pub const MIST2: Color32 = Color32::from_rgb(185, 174, 178);
/// The light: pulse red.
pub const PULSE: Color32 = Color32::from_rgb(255, 46, 77);
/// The light: coral halo (also the raw headphone curve).
pub const HALO: Color32 = Color32::from_rgb(255, 122, 92);
/// The light: sun gold (live dot, highlights).
pub const SOLEIL: Color32 = Color32::from_rgb(255, 197, 110);
/// The light: wine depth.
pub const DEPTH: Color32 = Color32::from_rgb(58, 10, 22);
/// Warnings: paused, blocked, disconnected.
pub const AMBER: Color32 = Color32::from_rgb(255, 154, 31);
/// Errors and clipping.
pub const ERROR: Color32 = Color32::from_rgb(255, 84, 84);

/// `c` with opacity `a` (0 to 1).
pub fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Frosted-glass fill (white 6 %).
pub fn glass_fill() -> Color32 {
    alpha(Color32::WHITE, 0.06)
}

/// Frosted-glass edge (white 13 %).
pub fn glass_edge() -> Color32 {
    alpha(Color32::WHITE, 0.13)
}

fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

/// Syne ExtraBold: logo, big titles.
pub fn display(size: f32) -> FontId {
    FontId::new(size, family("display"))
}

/// Syne Bold: state titles.
pub fn title(size: f32) -> FontId {
    FontId::new(size, family("title"))
}

/// Outfit ExtraLight: huge numbers.
pub fn thin(size: f32) -> FontId {
    FontId::new(size, family("thin"))
}

/// Outfit Light: long copy.
pub fn light(size: f32) -> FontId {
    FontId::new(size, family("light"))
}

/// Outfit Regular: interface text.
pub fn body(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Outfit Medium: labels, buttons.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, family("medium"))
}

/// Monospace (code chips, EQ export).
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

const SYNE: &[u8] = include_bytes!("../../assets/fonts/Syne-Variable.ttf");
const OUTFIT: &[u8] = include_bytes!("../../assets/fonts/Outfit-Variable.ttf");

fn instance(bytes: &'static [u8], weight: f32) -> Arc<FontData> {
    let tweak = FontTweak { coords: VariationCoords::new([(b"wght", weight)]), ..Default::default() };
    Arc::new(FontData::from_static(bytes).tweak(tweak))
}

/// Installs the fonts and the dark Horizon style.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let faces = [
        ("syne-800", SYNE, 800.0, family("display")),
        ("syne-700", SYNE, 700.0, family("title")),
        ("outfit-200", OUTFIT, 200.0, family("thin")),
        ("outfit-300", OUTFIT, 300.0, family("light")),
        ("outfit-400", OUTFIT, 400.0, FontFamily::Proportional),
        ("outfit-500", OUTFIT, 500.0, family("medium")),
    ];
    // Fallbacks (symbols, accents Outfit lacks) come from egui's defaults.
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, bytes, weight, fam) in faces {
        fonts.font_data.insert(name.into(), instance(bytes, weight));
        let list = fonts.families.entry(fam).or_default();
        list.insert(0, name.into());
        for f in &fallback {
            if !list.contains(f) {
                list.push(f.clone());
            }
        }
    }
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNSMono.ttf") {
        fonts.font_data.insert("sf-mono".into(), Arc::new(FontData::from_owned(bytes)));
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "sf-mono".into());
    }
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, body(12.0)),
            (TextStyle::Body, body(14.5)),
            (TextStyle::Button, medium(14.0)),
            (TextStyle::Heading, title(22.0)),
            (TextStyle::Monospace, mono(12.5)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(14.0, 8.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.combo_width = 220.0;

        let mut v = Visuals::dark();
        v.panel_fill = NIGHT;
        v.window_fill = Color32::from_rgb(24, 20, 26);
        v.window_stroke = Stroke::new(1.0, glass_edge());
        v.window_corner_radius = CornerRadius::same(18);
        v.menu_corner_radius = CornerRadius::same(16);
        v.extreme_bg_color = Color32::from_rgb(14, 12, 17);
        v.faint_bg_color = glass_fill();
        v.selection.bg_fill = alpha(PULSE, 0.3);
        v.selection.stroke = Stroke::new(1.0, MILK);
        v.hyperlink_color = SOLEIL;
        let r = CornerRadius::same(14);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = glass_fill();
        w.noninteractive.bg_stroke = Stroke::new(1.0, glass_edge());
        w.noninteractive.fg_stroke = Stroke::new(1.0, MIST);
        w.noninteractive.corner_radius = r;
        for (state, fill, edge) in
            [(&mut w.inactive, 0.07, 0.15), (&mut w.hovered, 0.12, 0.30), (&mut w.active, 0.18, 0.45)]
        {
            state.bg_fill = alpha(Color32::WHITE, fill);
            state.weak_bg_fill = alpha(Color32::WHITE, fill);
            state.bg_stroke = Stroke::new(1.0, alpha(Color32::WHITE, edge));
            state.fg_stroke = Stroke::new(1.0, MILK);
            state.corner_radius = r;
        }
        w.open = w.active;
        style.visuals = v;
    });
}
