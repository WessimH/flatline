//! Hand-drawn widgets.

use super::theme::*;
use eframe::egui::{self, Align2, Color32, CornerRadius, Frame, Margin, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
use std::f32::consts::PI;

/// A rounded card with the standard padding; its content is laid out vertically.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Always stack vertically, whatever the parent layout is.
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add).inner
        })
        .inner
}

/// Small, letter-spaced section title.
pub fn section_title(ui: &mut Ui, text: &str) {
    let spaced: String = text.chars().flat_map(|c| [c, '\u{2009}']).collect();
    ui.label(egui::RichText::new(spaced).font(prop(10.5)).color(FAINT).strong());
    ui.add_space(2.0);
}

/// Slider with label + value on top, glowing track below.
/// Double-click resets to `default`. Returns true when the value changed.
pub fn slider(
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
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 40.0), Sense::click_and_drag());
    let track_y = rect.bottom() - 9.0;
    let track =
        Rect::from_min_max(Pos2::new(rect.left() + 7.0, track_y - 2.0), Pos2::new(rect.right() - 7.0, track_y + 2.0));
    let to_x = |v: f64| track.left() + ((v - lo) / (hi - lo)) as f32 * track.width();

    let old = *value;
    if resp.double_clicked() {
        *value = default;
    } else if (resp.dragged() || resp.drag_started() || resp.clicked())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
        let mut v = lo + t * (hi - lo);
        if step > 0.0 {
            v = (v / step).round() * step;
        }
        *value = v.clamp(lo, hi);
    }
    let changed = *value != old;

    let painter = ui.painter_at(rect.expand(6.0));
    let hot = resp.hovered() || resp.dragged();
    painter.text(
        rect.left_top() + Vec2::new(0.0, 1.0),
        Align2::LEFT_TOP,
        label,
        prop(13.0),
        if hot { TEXT } else { DIM },
    );
    painter.text(
        rect.right_top() + Vec2::new(0.0, 1.0),
        Align2::RIGHT_TOP,
        fmt(*value),
        mono(12.5),
        if *value == default { TEXT } else { ACCENT },
    );

    painter.rect_filled(track, CornerRadius::same(2), BORDER);
    // Bipolar ranges fill from zero, unipolar from the left.
    let zero = if lo < 0.0 && hi > 0.0 { to_x(0.0) } else { track.left() };
    let x = to_x(*value);
    let fill = Rect::from_min_max(Pos2::new(zero.min(x), track.top()), Pos2::new(zero.max(x), track.bottom()));
    painter.rect_filled(fill, CornerRadius::same(2), ACCENT);
    if lo < 0.0 && hi > 0.0 {
        painter.line_segment(
            [Pos2::new(zero, track.top() - 3.0), Pos2::new(zero, track.bottom() + 3.0)],
            Stroke::new(1.0, FAINT),
        );
    }
    let knob = Pos2::new(x, track_y);
    if hot {
        painter.circle_filled(knob, 12.0, alpha(ACCENT, 0.15));
    }
    painter.circle_filled(knob, 7.0, TEXT);
    painter.circle_stroke(knob, 7.0, Stroke::new(2.0, ACCENT));
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    changed
}

/// Pill-shaped toggle chip.
pub fn chip(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), prop(12.5), TEXT);
    let size = Vec2::new(galley.size().x + 22.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    let (fill, stroke, col) = if selected {
        (alpha(ACCENT, 0.16), ACCENT, ACCENT)
    } else if resp.hovered() {
        (CARD_HI, alpha(ACCENT, 0.4), TEXT)
    } else {
        (CARD_HI, BORDER, DIM)
    };
    p.rect_filled(rect, CornerRadius::same(13), fill);
    p.rect_stroke(rect, CornerRadius::same(13), Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, text, prop(12.5), col);
    resp
}

/// iOS-style switch with a label on the left.
pub fn switch(ui: &mut Ui, label: &str, on: &mut bool) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
    let p = ui.painter();
    p.text(rect.left_center(), Align2::LEFT_CENTER, label, prop(13.0), if resp.hovered() { TEXT } else { DIM });
    let sw = Rect::from_center_size(Pos2::new(rect.right() - 19.0, rect.center().y), Vec2::new(36.0, 20.0));
    let bg = lerp_color(BORDER, ACCENT, t);
    p.rect_filled(sw, CornerRadius::same(10), bg);
    let cx = egui::lerp(sw.left() + 10.0..=sw.right() - 10.0, t);
    p.circle_filled(Pos2::new(cx, sw.center().y), 8.0, if *on { BG } else { TEXT });
    resp.clicked()
}

/// Linear interpolation between two colours.
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// Big round power button (correction on/off).
pub fn power_button(ui: &mut Ui, on: bool, d: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(d), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, on, 0.18);
    let p = ui.painter();
    let c = rect.center();
    let r = d * 0.5 - 2.0;
    if t > 0.0 {
        for (k, a) in [(1.0, 0.10), (0.85, 0.12), (0.7, 0.14)] {
            p.circle_filled(c, r * (1.0 + 0.35 * k * t), alpha(ACCENT, a * t));
        }
    }
    p.circle_filled(c, r, lerp_color(CARD_HI, Color32::from_rgb(14, 40, 42), t));
    p.circle_stroke(c, r, Stroke::new(1.5, lerp_color(if resp.hovered() { DIM } else { BORDER }, ACCENT, t)));
    // Power glyph.
    let col = lerp_color(DIM, ACCENT, t);
    let gr = r * 0.42;
    let pts: Vec<Pos2> = (0..=40)
        .map(|i| {
            let a = -PI / 2.0 + 0.35 * PI + (i as f32 / 40.0) * (2.0 * PI - 0.7 * PI);
            c + Vec2::new(a.cos(), a.sin()) * gr
        })
        .collect();
    p.line(pts, Stroke::new(2.2, col));
    p.line_segment([c + Vec2::new(0.0, -gr - 2.0), c + Vec2::new(0.0, -gr * 0.15)], Stroke::new(2.2, col));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Small status pill with a dot.
pub fn pill(ui: &mut Ui, text: &str, color: Color32, pulse: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), mono(11.5), color);
    let size = Vec2::new(galley.size().x + 30.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(12), alpha(color, 0.10));
    p.rect_stroke(rect, CornerRadius::same(12), Stroke::new(1.0, alpha(color, 0.35)), egui::StrokeKind::Inside);
    let dot = Pos2::new(rect.left() + 12.0, rect.center().y);
    if pulse {
        let t = (ui.input(|i| i.time) * 2.0).sin() as f32 * 0.5 + 0.5;
        p.circle_filled(dot, 3.5 + 3.0 * t, alpha(color, 0.25 * (1.0 - t)));
        ui.ctx().request_repaint();
    }
    p.circle_filled(dot, 3.5, color);
    p.text(Pos2::new(rect.left() + 21.0, rect.center().y), Align2::LEFT_CENTER, text, mono(11.5), color);
    resp
}

/// The Flatline mark: a heartbeat that settles into a flat line.
pub fn logo(ui: &mut Ui, h: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(h * 1.5, h), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same((h * 0.28) as u8), Color32::from_rgb(14, 40, 42));
    let y = rect.center().y;
    let x = |f: f32| rect.left() + f * rect.width();
    let pts = vec![
        Pos2::new(x(0.12), y),
        Pos2::new(x(0.30), y),
        Pos2::new(x(0.38), y - h * 0.30),
        Pos2::new(x(0.48), y + h * 0.26),
        Pos2::new(x(0.56), y - h * 0.10),
        Pos2::new(x(0.62), y),
        Pos2::new(x(0.88), y),
    ];
    p.line(pts.clone(), Stroke::new(4.0, alpha(ACCENT, 0.25)));
    p.line(pts, Stroke::new(1.8, ACCENT));
}

/// Horizontal level meter in dBFS (−60..0) with peak-hold tick.
pub fn meter(ui: &mut Ui, label: &str, level_db: f32, hold_db: f32) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 14.0), Sense::hover());
    let p = ui.painter();
    p.text(rect.left_center(), Align2::LEFT_CENTER, label, mono(10.5), DIM);
    let bar = Rect::from_min_max(
        Pos2::new(rect.left() + 46.0, rect.top() + 3.0),
        Pos2::new(rect.right(), rect.bottom() - 3.0),
    );
    p.rect_filled(bar, CornerRadius::same(2), Color32::from_rgb(20, 25, 33));
    let to_x = |db: f32| bar.left() + ((db + 60.0) / 60.0).clamp(0.0, 1.0) * bar.width();
    // Segmented fill, colored by zone.
    let seg = 3.0;
    let mut x0 = bar.left();
    let end = to_x(level_db);
    while x0 < end {
        let x1 = (x0 + seg - 1.0).min(end);
        let db = (x0 - bar.left()) / bar.width() * 60.0 - 60.0;
        let c = if db > -3.0 {
            RED
        } else if db > -12.0 {
            AMBER
        } else {
            ACCENT
        };
        p.rect_filled(Rect::from_min_max(Pos2::new(x0, bar.top()), Pos2::new(x1, bar.bottom())), CornerRadius::ZERO, c);
        x0 += seg;
    }
    if hold_db > -60.0 {
        let hx = to_x(hold_db);
        p.line_segment([Pos2::new(hx, bar.top() - 1.0), Pos2::new(hx, bar.bottom() + 1.0)], Stroke::new(2.0, TEXT));
    }
}
