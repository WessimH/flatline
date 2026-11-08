//! Frequency-response chart, Horizon style: minimal grid, the corrected
//! curve glowing over a pulse-red gradient, the raw headphone curve in coral
//! and the target dotted. `bypass` (0 → 1) crossfades toward the raw curve.

use super::theme::*;
use super::widgets::lerp_color;
use eframe::egui::{self, Align2, Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use serde::{Deserialize, Serialize};

/// Curves to draw, all sampled on `grid`.
pub struct Curves<'a> {
    /// Frequencies in Hz.
    pub grid: &'a [f64],
    /// Headphone response, aligned to the target.
    pub measured: &'a [f32],
    /// Target response.
    pub target: &'a [f32],
    /// Correction EQ response.
    pub eq: &'a [f32],
    /// Headphone response after correction.
    pub result: &'a [f32],
}

/// Live analyzer data.
pub struct Rta<'a> {
    /// Frequencies in Hz.
    pub freqs: &'a [f64],
    /// Input levels in dBFS.
    pub pre: &'a [f32],
    /// Output levels in dBFS.
    pub post: &'a [f32],
}

/// Which layers of the graph are visible.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layers {
    /// Headphone response.
    pub measured: bool,
    /// Target.
    pub target: bool,
    /// Correction EQ.
    pub eq: bool,
    /// Corrected result.
    pub result: bool,
    /// Real-time analyzer underlay.
    pub rta: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Self { measured: true, target: true, eq: false, result: true, rta: true }
    }
}

const DB_TOP: f32 = 20.0;
const DB_BOT: f32 = -20.0;
const RTA_TOP: f32 = -6.0;
const RTA_BOT: f32 = -100.0;

/// Formats a frequency compactly (`95`, `1.5k`, `12k`).
pub fn fmt_freq(f: f64) -> String {
    if f >= 10_000.0 {
        format!("{:.0}k", f / 1000.0)
    } else if f >= 1000.0 {
        format!("{:.1}k", f / 1000.0).replace(".0k", "k")
    } else {
        format!("{f:.0}")
    }
}

fn area(painter: &egui::Painter, pts: &[Pos2], base_y: f32, top: Color32, bottom: Color32) {
    if pts.len() < 2 {
        return;
    }
    let mut m = Mesh::default();
    for (i, p) in pts.iter().enumerate() {
        m.colored_vertex(*p, top);
        m.colored_vertex(Pos2::new(p.x, base_y), bottom);
        if i > 0 {
            let k = i as u32 * 2;
            m.add_triangle(k - 2, k - 1, k);
            m.add_triangle(k - 1, k + 1, k);
        }
    }
    painter.add(Shape::mesh(m));
}

/// Polyline whose colour runs sun → milk → coral from left to right.
fn gradient_line(painter: &egui::Painter, pts: &[Pos2], width: f32, opacity: f32) {
    let n = pts.len().max(2) as f32;
    for (i, w) in pts.windows(2).enumerate() {
        let t = i as f32 / n;
        let col = if t < 0.5 { lerp_color(SOLEIL, MILK, t * 2.0) } else { lerp_color(MILK, HALO, (t - 0.5) * 2.0) };
        painter.line_segment([w[0], w[1]], Stroke::new(width, alpha(col, opacity)));
    }
}

/// Draws the chart in a `size` box, with a hover readout.
pub fn draw(ui: &mut Ui, size: Vec2, c: &Curves, rta: Option<&Rta>, layers: &Layers, bypass: f32) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let plot = Rect::from_min_max(rect.min, rect.max - Vec2::new(0.0, 24.0));
    let painter = ui.painter_at(rect);
    let span = (1000f64).log2();
    let fx = |f: f64| plot.left() + ((f / 20.0).log2() / span) as f32 * plot.width();
    let fy = |db: f32| plot.top() + (DB_TOP - db.clamp(DB_BOT - 3.0, DB_TOP + 3.0)) / (DB_TOP - DB_BOT) * plot.height();
    let ry = |db: f32| plot.top() + (RTA_TOP - db).clamp(0.0, RTA_TOP - RTA_BOT) / (RTA_TOP - RTA_BOT) * plot.height();

    for f in [100.0, 1000.0, 10_000.0] {
        let x = fx(f);
        painter.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(1.0, alpha(Color32::WHITE, 0.06)),
        );
    }
    painter.line_segment(
        [Pos2::new(plot.left(), fy(0.0)), Pos2::new(plot.right(), fy(0.0))],
        Stroke::new(1.0, alpha(Color32::WHITE, 0.2)),
    );
    for (f, label) in [(20.0, "20 Hz"), (100.0, "100"), (1000.0, "1k"), (10_000.0, "10k"), (20_000.0, "20k")] {
        let align = match label {
            "20 Hz" => Align2::LEFT_TOP,
            "20k" => Align2::RIGHT_TOP,
            _ => Align2::CENTER_TOP,
        };
        painter.text(Pos2::new(fx(f), plot.bottom() + 6.0), align, label, body(12.5), MIST);
    }

    let clip = painter.with_clip_rect(plot.expand(4.0));
    if let (true, Some(r)) = (layers.rta, rta) {
        let post: Vec<Pos2> = r.freqs.iter().zip(r.post).map(|(&f, &d)| Pos2::new(fx(f), ry(d))).collect();
        area(&clip, &post, plot.bottom(), alpha(Color32::WHITE, 0.045), alpha(Color32::WHITE, 0.0));
    }
    if c.measured.is_empty() {
        painter.text(plot.center(), Align2::CENTER_CENTER, "computing…", body(15.0), MIST);
        return;
    }
    let pts = |v: &[f32]| -> Vec<Pos2> { c.grid.iter().zip(v).map(|(&f, &d)| Pos2::new(fx(f), fy(d))).collect() };
    let on = 1.0 - bypass;

    if layers.target {
        for s in Shape::dashed_line(&pts(c.target), Stroke::new(1.6, alpha(Color32::WHITE, 0.55)), 1.5, 5.0) {
            clip.add(s);
        }
    }
    if layers.eq {
        clip.line(pts(c.eq), Stroke::new(1.4, alpha(MIST2, 0.6)));
    }
    if layers.measured {
        clip.line(pts(c.measured), Stroke::new(1.4 + 1.2 * bypass, alpha(HALO, 0.5 + 0.5 * bypass)));
    }
    if layers.result && on > 0.02 {
        let r = pts(c.result);
        area(&clip, &r, plot.bottom(), alpha(PULSE, 0.50 * on), alpha(PULSE, 0.0));
        clip.line(r.clone(), Stroke::new(10.0, alpha(PULSE, 0.35 * on)));
        gradient_line(&clip, &r, 2.8, on);
    }

    if let Some(pos) = resp.hover_pos().filter(|p| plot.contains(*p)) {
        let f = 20.0 * 2f64.powf(((pos.x - plot.left()) / plot.width()) as f64 * span);
        let i = c.grid.partition_point(|&g| g < f).min(c.grid.len() - 1);
        let x = fx(c.grid[i]);
        clip.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(1.0, alpha(Color32::WHITE, 0.25)),
        );
        let mut rows: Vec<(&str, Color32, f32)> = Vec::new();
        if layers.target {
            rows.push(("Target", MILK, c.target[i]));
        }
        if layers.measured {
            rows.push(("Your headphones", HALO, c.measured[i]));
        }
        if layers.eq {
            rows.push(("Correction", MIST2, c.eq[i]));
        }
        if layers.result {
            rows.push(("Result", SOLEIL, c.result[i]));
        }
        for (_, col, v) in &rows {
            clip.circle_filled(Pos2::new(x, fy(*v)), 4.0, *col);
        }
        let w = 200.0;
        let h = 34.0 + rows.len() as f32 * 20.0;
        let mut bx = pos.x + 16.0;
        if bx + w > plot.right() {
            bx = pos.x - 16.0 - w;
        }
        let by = (pos.y - h / 2.0).clamp(plot.top() + 4.0, plot.bottom() - h - 4.0);
        let r = Rect::from_min_size(Pos2::new(bx, by), Vec2::new(w, h));
        painter.rect_filled(r, CornerRadius::same(14), Color32::from_rgba_unmultiplied(28, 24, 30, 240));
        painter.rect_stroke(r, CornerRadius::same(14), Stroke::new(1.0, glass_edge()), StrokeKind::Inside);
        painter.text(
            r.left_top() + Vec2::new(14.0, 10.0),
            Align2::LEFT_TOP,
            format!("{} Hz", fmt_freq(c.grid[i])),
            medium(14.0),
            MILK,
        );
        for (k, (name, col, v)) in rows.iter().enumerate() {
            let y = r.top() + 34.0 + k as f32 * 20.0;
            painter.circle_filled(Pos2::new(r.left() + 18.0, y + 7.0), 3.5, *col);
            painter.text(Pos2::new(r.left() + 28.0, y), Align2::LEFT_TOP, *name, body(13.0), MIST);
            painter.text(Pos2::new(r.right() - 14.0, y), Align2::RIGHT_TOP, format!("{v:+.1} dB"), medium(13.0), *col);
        }
    }
}
