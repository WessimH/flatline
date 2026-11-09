//! Hand-drawn "Horizon" widgets: glass, pills, round buttons, the power
//! button, the Amount fill slider, the Bass dial, level bars and the logo.

use super::theme::*;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, Frame, Margin, Mesh, Painter, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2, text::LayoutJob,
};
use std::f32::consts::PI;

/// Linear interpolation between two colours.
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// Frosted-glass card frame (fill, edge, 28 px radius).
pub fn glass_frame() -> Frame {
    Frame::new()
        .fill(glass_fill())
        .stroke(Stroke::new(1.0, glass_edge()))
        .corner_radius(CornerRadius::same(28))
        .inner_margin(Margin::same(22))
}

/// Paints a glass panel at `rect`.
pub fn paint_glass(painter: &Painter, rect: Rect, radius: u8) {
    painter.rect_filled(rect, CornerRadius::same(radius), glass_fill());
    painter.rect_stroke(rect, CornerRadius::same(radius), Stroke::new(1.0, glass_edge()), StrokeKind::Inside);
}

/// Small letter-spaced caption ("NOW CALIBRATING").
pub fn eyebrow(text: &str, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat { font_id: medium(12.0), color, extra_letter_spacing: 2.6, ..Default::default() },
    );
    job
}

/// Text with tightened tracking (display titles).
pub fn tight(text: &str, font: egui::FontId, color: Color32, tracking: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let spacing = tracking * font.size;
    job.append(
        text,
        0.0,
        egui::TextFormat { font_id: font, color, extra_letter_spacing: spacing, ..Default::default() },
    );
    job
}

/// Pill button: glass, or milk white when selected.
pub fn pill(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), medium(14.5), MILK);
    let size = Vec2::new(galley.size().x + 40.0, 42.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, selected, 0.15);
    let hover = if resp.hovered() { 0.05 } else { 0.0 };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(21), lerp_color(alpha(Color32::WHITE, 0.07 + hover), MILK, t));
    if t < 1.0 {
        p.rect_stroke(
            rect,
            CornerRadius::same(21),
            Stroke::new(1.0, alpha(Color32::WHITE, 0.16 * (1.0 - t))),
            StrokeKind::Inside,
        );
    }
    p.text(rect.center(), Align2::CENTER_CENTER, text, medium(14.5), lerp_color(MILK, NIGHT, t));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Small glass tag ("Change", "Recommended").
pub fn tag(ui: &mut Ui, text: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), medium(12.5), MILK);
    let size = Vec2::new(galley.size().x + 24.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(14), alpha(Color32::WHITE, if resp.hovered() { 0.14 } else { 0.08 }));
    p.rect_stroke(rect, CornerRadius::same(14), Stroke::new(1.0, alpha(Color32::WHITE, 0.18)), StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, text, medium(12.5), MILK);
    resp
}

/// Line icons used across the interface.
#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    /// Output device.
    Headphones,
    /// Settings.
    Sliders,
    /// Chevron pointing down.
    ChevronDown,
    /// Chevron pointing up.
    ChevronUp,
    /// Close.
    Close,
    /// Add.
    Plus,
    /// Copy.
    Copy,
    /// Loudspeaker (the Mac's built-in speakers).
    Speaker,
    /// Padlock.
    Lock,
    /// Warning triangle.
    Warning,
    /// Moon (sleep).
    Moon,
    /// Magnifier.
    Search,
    /// Ear (listening permission).
    Ear,
    /// Check mark.
    Check,
    /// Document (measurement file).
    File,
}

/// Draws `icon` centred at `c`, `s` px wide.
pub fn draw_icon(p: &Painter, icon: Icon, c: Pos2, s: f32, color: Color32) {
    let st = Stroke::new((s * 0.09).max(1.4), color);
    let at = |x: f32, y: f32| c + Vec2::new(x * s, y * s);
    match icon {
        Icon::Headphones => {
            let arc: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = PI + i as f32 / 24.0 * PI;
                    at(0.36 * a.cos(), 0.36 * a.sin() + 0.08)
                })
                .collect();
            p.line(arc, st);
            for x in [-0.36, 0.24] {
                let r = Rect::from_min_size(at(x, 0.06), Vec2::new(0.12 * s, 0.30 * s));
                p.rect_stroke(r, CornerRadius::same((s * 0.05) as u8), st, StrokeKind::Middle);
            }
        }
        Icon::Sliders => {
            for (y, k) in [(-0.2, -0.12), (0.2, 0.14)] {
                p.line_segment([at(-0.38, y), at(0.38, y)], st);
                p.circle_filled(at(k, y), s * 0.12, color);
                p.circle_filled(at(k, y), s * 0.06, MILK);
            }
        }
        Icon::ChevronDown => {
            p.line(vec![at(-0.22, -0.08), at(0.0, 0.14), at(0.22, -0.08)], st);
        }
        Icon::ChevronUp => {
            p.line(vec![at(-0.22, 0.10), at(0.0, -0.12), at(0.22, 0.10)], st);
        }
        Icon::Close => {
            p.line_segment([at(-0.22, -0.22), at(0.22, 0.22)], st);
            p.line_segment([at(0.22, -0.22), at(-0.22, 0.22)], st);
        }
        Icon::Plus => {
            p.line_segment([at(-0.24, 0.0), at(0.24, 0.0)], st);
            p.line_segment([at(0.0, -0.24), at(0.0, 0.24)], st);
        }
        Icon::Copy => {
            p.rect_stroke(
                Rect::from_min_size(at(-0.28, -0.14), Vec2::splat(0.38 * s)),
                CornerRadius::same(2),
                st,
                StrokeKind::Middle,
            );
            p.line(vec![at(-0.12, -0.28), at(0.26, -0.28), at(0.26, 0.10)], st);
        }
        Icon::Speaker => {
            p.line(
                vec![
                    at(-0.34, -0.10),
                    at(-0.18, -0.10),
                    at(0.0, -0.28),
                    at(0.0, 0.28),
                    at(-0.18, 0.10),
                    at(-0.34, 0.10),
                    at(-0.34, -0.10),
                ],
                st,
            );
            for r in [0.16, 0.30] {
                let arc: Vec<Pos2> = (0..=12)
                    .map(|i| {
                        let a = -0.3 * PI + i as f32 / 12.0 * 0.6 * PI;
                        at(0.06 + r * a.cos(), r * a.sin())
                    })
                    .collect();
                p.line(arc, st);
            }
        }
        Icon::Lock => {
            p.rect_stroke(
                Rect::from_min_max(at(-0.28, -0.04), at(0.28, 0.34)),
                CornerRadius::same(3),
                st,
                StrokeKind::Middle,
            );
            let arc: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = PI + i as f32 / 16.0 * PI;
                    at(0.18 * a.cos(), 0.18 * a.sin() - 0.04)
                })
                .collect();
            p.line(arc, st);
        }
        Icon::Warning => {
            p.line(vec![at(0.0, -0.34), at(0.36, 0.30), at(-0.36, 0.30), at(0.0, -0.34)], st);
            p.line_segment([at(0.0, -0.08), at(0.0, 0.10)], st);
            p.circle_filled(at(0.0, 0.20), s * 0.035, color);
        }
        Icon::Moon => {
            let pts: Vec<Pos2> = (0..=28)
                .map(|i| {
                    let a = -0.6 * PI + i as f32 / 28.0 * 1.6 * PI;
                    at(0.30 * a.cos(), 0.30 * a.sin())
                })
                .collect();
            p.line(pts, st);
        }
        Icon::Search => {
            p.circle_stroke(at(-0.06, -0.06), 0.22 * s, st);
            p.line_segment([at(0.10, 0.10), at(0.30, 0.30)], st);
        }
        Icon::Ear => {
            // Outer helix, then the inner fold.
            let helix: Vec<Pos2> = (0..=28)
                .map(|i| {
                    let a = 0.95 * PI + i as f32 / 28.0 * 1.25 * PI;
                    at(0.24 * a.cos(), 0.26 * a.sin() - 0.06)
                })
                .collect();
            let end = *helix.last().unwrap_or(&c);
            p.line(helix, st);
            p.line(vec![end, at(0.10, 0.22), at(0.0, 0.36), at(-0.12, 0.32)], st);
            let fold: Vec<Pos2> = (0..=14)
                .map(|i| {
                    let a = 1.05 * PI + i as f32 / 14.0 * 1.1 * PI;
                    at(0.10 * a.cos() + 0.01, 0.11 * a.sin() - 0.04)
                })
                .collect();
            p.line(fold, st);
        }
        Icon::Check => {
            p.line(vec![at(-0.26, 0.0), at(-0.08, 0.18), at(0.28, -0.20)], st);
        }
        Icon::File => {
            p.line(
                vec![
                    at(0.08, -0.34),
                    at(-0.24, -0.34),
                    at(-0.24, 0.34),
                    at(0.26, 0.34),
                    at(0.26, -0.16),
                    at(0.08, -0.34),
                ],
                st,
            );
            p.line(vec![at(0.08, -0.34), at(0.08, -0.16), at(0.26, -0.16)], st);
            p.line(vec![at(-0.12, 0.18), at(-0.02, 0.0), at(0.06, 0.10), at(0.14, -0.02)], st);
        }
    }
}

/// White round icon button (header, close).
pub fn icon_button(ui: &mut Ui, icon: Icon, d: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(d), Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { Color32::WHITE } else { MILK };
    p.circle_filled(rect.center(), d / 2.0, fill);
    draw_icon(p, icon, rect.center(), d * 0.46, NIGHT);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Glass round icon button (chevron next to the headphone title).
pub fn glass_icon_button(ui: &mut Ui, icon: Icon, d: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(d), Sense::click());
    let p = ui.painter();
    p.circle_filled(rect.center(), d / 2.0, alpha(Color32::WHITE, if resp.hovered() { 0.16 } else { 0.09 }));
    p.circle_stroke(rect.center(), d / 2.0, Stroke::new(1.0, alpha(Color32::WHITE, 0.2)));
    draw_icon(p, icon, rect.center(), d * 0.5, MILK);
    resp
}

/// The big round correction button: milk white with pulsing rings when on,
/// dark glass when bypassed.
pub fn power_button(ui: &mut Ui, on: bool, d: f32, time: f64) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(d * 1.15), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, on, 0.32);
    let p = ui.painter();
    let c = rect.center();
    let r = d / 2.0;
    if t > 0.01 {
        for phase in [0.0, 0.5] {
            let k = ((time as f32 / 2.6 + phase) % 1.0).clamp(0.0, 1.0);
            p.circle_stroke(c, r * (1.0 + 0.62 * k), Stroke::new(2.0, alpha(SOLEIL, 0.5 * (1.0 - k) * t)));
        }
        for (gr, ga) in [(1.35, 0.08), (1.15, 0.12)] {
            p.circle_filled(c, r * gr, alpha(PULSE, ga * t));
        }
        ui.ctx().request_repaint();
    }
    p.circle_filled(c, r, lerp_color(alpha(Color32::WHITE, 0.06), MILK, t));
    p.circle_stroke(c, r, Stroke::new(1.5, alpha(Color32::WHITE, 0.3 * (1.0 - t))));
    let col = lerp_color(MILK, PULSE, t);
    let gr = r * 0.30;
    let st = Stroke::new(r * 0.065, col);
    let arc: Vec<Pos2> = (0..=40)
        .map(|i| {
            let a = -PI / 2.0 + 0.32 * PI + i as f32 / 40.0 * (2.0 * PI - 0.64 * PI);
            c + Vec2::new(a.cos(), a.sin()) * gr
        })
        .collect();
    p.line(arc, st);
    p.line_segment([c + Vec2::new(0.0, -gr * 1.15), c + Vec2::new(0.0, -gr * 0.2)], st);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Tone of a status pill.
#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    /// Running: sun dot, pulsing.
    Live,
    /// Neutral: bypass, standby.
    Neutral,
    /// Amber: paused, blocked, disconnected.
    Warning,
    /// Red: errors.
    Error,
}

/// Glass status pill with a dot.
pub fn status_pill(ui: &mut Ui, text: &str, tone: Tone, time: f64) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), body(14.0), MILK);
    let size = Vec2::new(galley.size().x + 50.0, 40.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    let (dot, fill, edge) = match tone {
        Tone::Live => (SOLEIL, alpha(Color32::WHITE, 0.08), alpha(Color32::WHITE, 0.15)),
        Tone::Neutral => (MIST2, alpha(Color32::WHITE, 0.07), alpha(Color32::WHITE, 0.14)),
        Tone::Warning => (AMBER, alpha(AMBER, 0.10), alpha(AMBER, 0.42)),
        Tone::Error => (ERROR, alpha(ERROR, 0.10), alpha(ERROR, 0.45)),
    };
    p.rect_filled(rect, CornerRadius::same(20), fill);
    p.rect_stroke(rect, CornerRadius::same(20), Stroke::new(1.0, edge), StrokeKind::Inside);
    let dc = Pos2::new(rect.left() + 20.0, rect.center().y);
    if tone == Tone::Live {
        let k = ((time / 1.8) % 1.0) as f32;
        p.circle_filled(dc, 4.0 + 7.0 * k, alpha(dot, 0.35 * (1.0 - k)));
        ui.ctx().request_repaint();
    }
    p.circle_filled(dc, 4.0, dot);
    p.text(Pos2::new(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, text, body(14.0), MILK);
    resp
}

/// iOS-style switch with a label (settings sheet).
pub fn switch(ui: &mut Ui, label: &str, on: &mut bool) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 30.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.14);
    let p = ui.painter();
    p.text(rect.left_center(), Align2::LEFT_CENTER, label, body(14.5), if resp.hovered() { MILK } else { MIST });
    let sw = Rect::from_center_size(Pos2::new(rect.right() - 22.0, rect.center().y), Vec2::new(44.0, 26.0));
    p.rect_filled(sw, CornerRadius::same(13), lerp_color(alpha(Color32::WHITE, 0.18), PULSE, t));
    let cx = egui::lerp(sw.left() + 13.0..=sw.right() - 13.0, t);
    p.circle_filled(Pos2::new(cx, sw.center().y), 10.0, MILK);
    resp.clicked()
}

/// Horizontal slider for the settings sheet. Double-click resets.
pub fn hslider(
    ui: &mut Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    default: f64,
    step: f64,
    fmt: impl Fn(f64) -> String,
) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 46.0), Sense::click_and_drag());
    let ty = rect.bottom() - 10.0;
    let track = Rect::from_min_max(Pos2::new(rect.left() + 9.0, ty - 2.0), Pos2::new(rect.right() - 9.0, ty + 2.0));
    let to_x = |v: f64| track.left() + ((v - lo) / (hi - lo)) as f32 * track.width();
    let old = *value;
    if resp.double_clicked() {
        *value = default;
    } else if (resp.dragged() || resp.clicked())
        && let Some(pt) = resp.interact_pointer_pos()
    {
        let t = ((pt.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
        let v = lo + t * (hi - lo);
        *value = if step > 0.0 { (v / step).round() * step } else { v }.clamp(lo, hi);
    }
    let p = ui.painter_at(rect.expand(8.0));
    let hot = resp.hovered() || resp.dragged();
    p.text(rect.left_top(), Align2::LEFT_TOP, label, body(14.0), if hot { MILK } else { MIST });
    p.text(
        rect.right_top(),
        Align2::RIGHT_TOP,
        fmt(*value),
        medium(14.0),
        if *value == default { MILK } else { SOLEIL },
    );
    p.rect_filled(track, CornerRadius::same(2), alpha(Color32::WHITE, 0.14));
    let zero = if lo < 0.0 && hi > 0.0 { to_x(0.0) } else { track.left() };
    let x = to_x(*value);
    let fill = Rect::from_min_max(Pos2::new(zero.min(x), track.top()), Pos2::new(zero.max(x), track.bottom()));
    p.rect_filled(fill, CornerRadius::same(2), MILK);
    p.circle_filled(Pos2::new(x, ty), if hot { 9.0 } else { 8.0 }, MILK);
    *value != old
}

/// The Amount card body: a tall glass track filled in milk white from the
/// bottom, a grip, and the value written huge inside the fill.
pub fn amount_slider(ui: &mut Ui, size: Vec2, value: &mut f64, default: f64) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let old = *value;
    if resp.double_clicked() {
        *value = default;
    } else if (resp.dragged() || resp.clicked())
        && let Some(pt) = resp.interact_pointer_pos()
    {
        *value = (((rect.bottom() - 4.0 - pt.y) / (rect.height() - 8.0)) as f64).clamp(0.0, 1.0);
        *value = (*value * 100.0).round() / 100.0;
    }
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(22), alpha(Color32::WHITE, 0.05));
    let h = ((rect.height() - 8.0) * *value as f32).max(30.0);
    let fill = Rect::from_min_max(
        Pos2::new(rect.left() + 4.0, rect.bottom() - 4.0 - h),
        Pos2::new(rect.right() - 4.0, rect.bottom() - 4.0),
    );
    // Soft glow above the fill.
    p.rect_filled(fill.expand(6.0), CornerRadius::same(24), alpha(PULSE, 0.10));
    let mut m = Mesh::default();
    m.colored_vertex(fill.left_top(), Color32::WHITE);
    m.colored_vertex(fill.right_top(), Color32::WHITE);
    m.colored_vertex(fill.right_bottom(), Color32::from_rgb(230, 220, 214));
    m.colored_vertex(fill.left_bottom(), Color32::from_rgb(230, 220, 214));
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.rect_filled(fill, CornerRadius::same(19), Color32::from_rgb(236, 228, 223));
    p.add(Shape::mesh(m));
    p.rect_stroke(fill, CornerRadius::same(19), Stroke::new(3.0, Color32::from_rgb(236, 228, 223)), StrokeKind::Inside);
    let grip = Rect::from_center_size(Pos2::new(fill.center().x, fill.top() + 12.0), Vec2::new(36.0, 6.0));
    p.rect_filled(grip, CornerRadius::same(3), alpha(NIGHT, 0.55));
    let pct = format!("{:.0}", *value * 100.0);
    let num = p.layout_no_wrap(pct, thin(78.0), NIGHT);
    let unit = p.layout_no_wrap("%".into(), body(22.0), NIGHT);
    let base = Pos2::new(fill.right() - 16.0 - unit.size().x, fill.bottom() - 8.0);
    p.galley(Pos2::new(base.x - num.size().x, base.y - num.size().y), num.clone(), NIGHT);
    p.galley(Pos2::new(base.x + 2.0, base.y - unit.size().y - 12.0), unit, NIGHT);
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    *value != old
}

/// The Bass dial: tick ring, gradient arc, knob and the value in the middle.
/// Drag up/down (or sideways) to change; double-click resets.
pub fn dial(ui: &mut Ui, size: Vec2, value: &mut f64, range: std::ops::RangeInclusive<f64>, default: f64) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let old = *value;
    if resp.double_clicked() {
        *value = default;
    } else if resp.dragged() {
        let d = resp.drag_delta();
        let delta = (d.x - d.y) as f64 * (hi - lo) / 260.0;
        *value = ((*value + delta) * 2.0).round().clamp(lo * 2.0, hi * 2.0) / 2.0;
    }
    let p = ui.painter();
    let r = (rect.width().min(rect.height() * 1.15) * 0.40).max(20.0);
    let c = Pos2::new(rect.center().x, rect.top() + r + 18.0);
    let (a0, sweep) = (PI * 150.0 / 180.0, PI * 240.0 / 180.0);
    let pt = |a: f32, rr: f32| c + Vec2::new(a.cos(), a.sin()) * rr;
    // Ticks.
    for i in 0..=40 {
        let a = a0 + sweep * i as f32 / 40.0;
        p.line_segment([pt(a, r * 1.13), pt(a, r * 1.21)], Stroke::new(1.4, alpha(Color32::WHITE, 0.28)));
    }
    let arc = |from: f32, to: f32, steps: usize| -> Vec<Pos2> {
        (0..=steps).map(|i| pt(from + (to - from) * i as f32 / steps as f32, r)).collect()
    };
    p.line(arc(a0, a0 + sweep, 64), Stroke::new(10.0, alpha(Color32::WHITE, 0.10)));
    let t = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0) as f32;
    let n = (64.0 * t).ceil().max(1.0) as usize;
    let pts = arc(a0, a0 + sweep * t, n);
    for w in pts.windows(2).enumerate() {
        let k = w.0 as f32 / n as f32 * t;
        p.line_segment([w.1[0], w.1[1]], Stroke::new(10.0, lerp_color(PULSE, SOLEIL, k)));
    }
    p.circle_filled(pts[0], 5.0, PULSE);
    let knob = pt(a0 + sweep * t, r);
    p.circle_filled(knob + Vec2::new(0.0, 2.0), 12.0, alpha(Color32::BLACK, 0.35));
    p.circle_filled(knob, 11.0, MILK);
    p.text(c + Vec2::new(0.0, 4.0), Align2::CENTER_CENTER, format!("{:+.1}", *value), thin(50.0), MILK);
    p.text(c + Vec2::new(0.0, 36.0), Align2::CENTER_CENTER, "dB", medium(13.0), MIST);
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    *value != old
}

/// Live level bars, coloured pulse → sun (or grey when inactive).
pub fn bars(p: &Painter, rect: Rect, levels: &[f32], active: f32) {
    if levels.is_empty() {
        return;
    }
    let n = levels.len() as f32;
    let gap = 4.0;
    let bw = ((rect.width() - gap * (n - 1.0)) / n).max(1.0);
    for (i, &l) in levels.iter().enumerate() {
        let h = (rect.height() * l.clamp(0.03, 1.0)).max(4.0);
        let x = rect.left() + i as f32 * (bw + gap);
        let r = Rect::from_min_max(Pos2::new(x, rect.bottom() - h), Pos2::new(x + bw, rect.bottom()));
        let top = lerp_color(alpha(Color32::WHITE, 0.45), SOLEIL, active);
        let bottom = lerp_color(alpha(Color32::WHITE, 0.30), PULSE, active);
        let mut m = Mesh::default();
        m.colored_vertex(r.left_top(), top);
        m.colored_vertex(r.right_top(), top);
        m.colored_vertex(r.right_bottom(), bottom);
        m.colored_vertex(r.left_bottom(), bottom);
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        p.add(Shape::mesh(m));
    }
}

/// A flat line with a heartbeat blip travelling along it (Latency card).
pub fn heartbeat(p: &Painter, rect: Rect, time: f64, color: Color32) {
    let y = rect.center().y;
    p.line_segment(
        [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
        Stroke::new(2.0, alpha(Color32::WHITE, 0.32)),
    );
    let w = rect.width();
    let k = ((time / 3.2) % 1.0) as f32;
    let x0 = rect.left() - 40.0 + (w + 80.0) * k;
    let s = rect.height() * 0.45;
    let blip = [(0.0, 0.0), (10.0, 0.0), (14.0, -s), (19.0, s * 0.85), (23.0, 0.0), (34.0, 0.0)];
    let pts: Vec<Pos2> =
        blip.iter().map(|&(dx, dy)| Pos2::new((x0 + dx).clamp(rect.left(), rect.right()), y + dy)).collect();
    p.line(pts, Stroke::new(2.4, color));
}

// ------------------------------------------------------------- logo ------

/// Approved symbol geometry (unit = orb radius, origin = orb centre, y down).
const CENTRELINE: [(f32, f32); 6] =
    [(-2.344, 0.3), (-1.964, 0.3), (-1.744, -0.45), (-1.444, 0.70), (-1.274, 0.3), (1.874, 0.3)];

fn gradient_at(d: f32) -> Color32 {
    // Radial stops of the orb (distance / radius 1.62).
    let stops = [(0.0, SOLEIL), (0.07, SOLEIL), (0.40, HALO), (0.71, PULSE), (1.0, DEPTH)];
    for w in stops.windows(2) {
        let ((t0, c0), (t1, c1)) = (w[0], w[1]);
        if d <= t1 {
            return lerp_color(c0, c1, (d - t0) / (t1 - t0));
        }
    }
    DEPTH
}

/// Paints the Flatline symbol (orb setting on the horizon, last heartbeat on
/// the left) with orb radius `r`, the orb centre at `c`. `mono` draws it in
/// one colour.
pub fn paint_symbol(p: &Painter, c: Pos2, r: f32, mono: Option<Color32>) {
    let at = |x: f32, y: f32| c + Vec2::new(x * r, y * r);
    // Upper orb: circle clipped to y <= 0.07, radial gradient centred at (0, -0.5).
    let ycut = 0.07f32;
    let a_cut = ycut.asin();
    let n = 48;
    // Upper arc from the left cut (θ = π − a), over the top, to the right cut (θ = 2π + a).
    let rim: Vec<(f32, f32)> = (0..=n)
        .map(|i| {
            let th = (PI - a_cut) + (PI + 2.0 * a_cut) * (i as f32 / n as f32);
            (th.cos(), th.sin())
        })
        .collect();
    let g = (0.0f32, -0.5f32);
    let mut m = Mesh::default();
    let color = |x: f32, y: f32| match mono {
        Some(col) => col,
        None => gradient_at(((x - g.0).powi(2) + (y - g.1).powi(2)).sqrt() / 1.62),
    };
    // Rings between the gradient centre and the rim for a smooth radial fill.
    let ks = [0.0, 0.3, 0.6, 0.85, 1.0];
    for &k in &ks {
        for &(x, y) in &rim {
            let (px, py) = (g.0 + (x - g.0) * k, g.1 + (y - g.1) * k);
            m.colored_vertex(at(px, py), color(px, py));
        }
    }
    let rn = rim.len() as u32;
    for ring in 0..ks.len() as u32 - 1 {
        for i in 0..rn - 1 {
            let (a, b) = (ring * rn + i, (ring + 1) * rn + i);
            m.add_triangle(a, a + 1, b);
            m.add_triangle(a + 1, b + 1, b);
        }
    }
    // Close the cap along the cut line.
    let left = at(rim[0].0, rim[0].1);
    let right = at(rim[rim.len() - 1].0, rim[rim.len() - 1].1);
    let base = m.vertices.len() as u32;
    m.colored_vertex(at(g.0, g.1), color(g.0, g.1));
    m.colored_vertex(left, color(rim[0].0, rim[0].1));
    m.colored_vertex(right, color(rim[rim.len() - 1].0, rim[rim.len() - 1].1));
    m.add_triangle(base, base + 1, base + 2);
    p.add(Shape::mesh(m));
    // Reflection: circle clipped to y >= 0.53, fading downwards.
    let y0 = 0.53f32;
    let refl_col = |y: f32| match mono {
        Some(col) => alpha(col, 0.35),
        None => alpha(PULSE, 0.5 - 0.46 * ((y - y0) / (1.0 - y0))),
    };
    let half = (1.0 - y0 * y0).sqrt();
    let steps = 24;
    let mut m = Mesh::default();
    for i in 0..=steps {
        let x = -half + 2.0 * half * i as f32 / steps as f32;
        let ybot = (1.0 - x * x).max(0.0).sqrt();
        m.colored_vertex(at(x, y0), refl_col(y0));
        m.colored_vertex(at(x, ybot), refl_col(ybot));
    }
    for i in 0..steps as u32 {
        let k = i * 2;
        m.add_triangle(k, k + 1, k + 2);
        m.add_triangle(k + 1, k + 3, k + 2);
    }
    p.add(Shape::mesh(m));
    // Horizon line with the heartbeat.
    let line_col = mono.unwrap_or(MILK);
    let pts: Vec<Pos2> = CENTRELINE.iter().map(|&(x, y)| at(x, y)).collect();
    p.line(pts, Stroke::new(0.22 * r, line_col));
    for &(x, y) in [CENTRELINE[0], CENTRELINE[5]].iter() {
        p.circle_filled(at(x, y), 0.11 * r, line_col);
    }
}

/// Primary lockup: symbol + "flatline" wordmark, `font` = wordmark size.
pub fn logo(ui: &mut Ui, font: f32) -> Response {
    let r = 0.538 * font;
    let sym_w = 4.44 * r;
    let job = tight("flatline", display(font), MILK, -0.05);
    let galley = ui.painter().layout_job(job);
    let size = Vec2::new(sym_w + 0.4 * font + galley.size().x, galley.size().y.max(2.0 * r));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    // Horizon line on the text baseline (approximate: 78 % of the row height).
    let baseline = rect.top() + galley.size().y * 0.80;
    let c = Pos2::new(rect.left() + 2.454 * r, baseline - 0.3 * r);
    paint_symbol(p, c, r, None);
    p.galley(Pos2::new(rect.left() + sym_w + 0.4 * font, rect.top()), galley, MILK);
    resp
}
