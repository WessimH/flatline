//! Frequency-response plot with live RTA underlay.

use super::theme::*;
use eframe::egui::{self, Align2, Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
/// Which layers of the graph are visible.
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
        Self { measured: true, target: true, eq: true, result: true, rta: true }
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

fn glow_line(painter: &egui::Painter, pts: Vec<Pos2>, color: Color32, width: f32) {
    painter.line(pts.clone(), Stroke::new(width * 4.0, alpha(color, 0.07)));
    painter.line(pts.clone(), Stroke::new(width * 2.2, alpha(color, 0.18)));
    painter.line(pts, Stroke::new(width, color));
}

/// Draws the frequency-response plot, with hover readout, in a `size` box.
pub fn draw(ui: &mut Ui, size: Vec2, c: &Curves, rta: Option<&Rta>, layers: &Layers, caption: &str) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let plot = Rect::from_min_max(rect.min + Vec2::new(34.0, 4.0), rect.max - Vec2::new(4.0, 22.0));
    let painter = ui.painter_at(rect);
    let span = (1000f64).log2();
    let fx = |f: f64| plot.left() + ((f / 20.0).log2() / span) as f32 * plot.width();
    let fy = |db: f32| plot.top() + (DB_TOP - db.clamp(DB_BOT - 3.0, DB_TOP + 3.0)) / (DB_TOP - DB_BOT) * plot.height();
    let ry = |db: f32| plot.top() + (RTA_TOP - db).clamp(0.0, RTA_TOP - RTA_BOT) / (RTA_TOP - RTA_BOT) * plot.height();

    painter.rect_filled(plot, CornerRadius::same(6), Color32::from_rgb(10, 13, 18));

    // Grid.
    let majors = [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0];
    for decade in [10.0, 100.0, 1000.0, 10_000.0] {
        for m in 1..10 {
            let f = decade * m as f64;
            if !(20.0..=20_000.0).contains(&f) {
                continue;
            }
            let x = fx(f);
            let major = majors.contains(&f);
            painter.line_segment(
                [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                Stroke::new(1.0, if major { GRID } else { alpha(GRID, 0.45) }),
            );
            if major {
                painter.text(Pos2::new(x, plot.bottom() + 5.0), Align2::CENTER_TOP, fmt_freq(f), mono(10.5), FAINT);
            }
        }
    }
    for db in (-20..=20).step_by(5) {
        let y = fy(db as f32);
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(1.0, if db == 0 { BORDER } else { GRID }),
        );
        painter.text(
            Pos2::new(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            format!("{db:+}").replace("+0", "0"),
            mono(10.5),
            FAINT,
        );
    }
    painter.text(plot.left_top() + Vec2::new(10.0, 8.0), Align2::LEFT_TOP, caption, prop(11.5), FAINT);

    let clip = painter.with_clip_rect(plot);
    let pts = |v: &[f32]| -> Vec<Pos2> { c.grid.iter().zip(v).map(|(&f, &d)| Pos2::new(fx(f), fy(d))).collect() };

    // Live analyzer underlay.
    if let (true, Some(r)) = (layers.rta, rta) {
        let post: Vec<Pos2> = r.freqs.iter().zip(r.post).map(|(&f, &d)| Pos2::new(fx(f), ry(d))).collect();
        let pre: Vec<Pos2> = r.freqs.iter().zip(r.pre).map(|(&f, &d)| Pos2::new(fx(f), ry(d))).collect();
        area(&clip, &post, plot.bottom(), alpha(ACCENT, 0.16), alpha(ACCENT, 0.0));
        clip.line(post, Stroke::new(1.0, alpha(ACCENT, 0.35)));
        clip.line(pre, Stroke::new(1.0, alpha(DIM, 0.30)));
    }

    if c.measured.is_empty() {
        painter.text(plot.center(), Align2::CENTER_CENTER, "computing…", prop(14.0), DIM);
        return;
    }

    if layers.eq {
        let p = pts(c.eq);
        area(&clip, &p, fy(0.0), alpha(VIOLET, 0.22), alpha(VIOLET, 0.02));
        clip.line(p, Stroke::new(1.6, alpha(VIOLET, 0.9)));
    }
    if layers.measured {
        clip.line(pts(c.measured), Stroke::new(1.6, alpha(AMBER, 0.85)));
    }
    if layers.target {
        for s in Shape::dashed_line(&pts(c.target), Stroke::new(1.4, alpha(TEXT, 0.55)), 6.0, 4.0) {
            clip.add(s);
        }
    }
    if layers.result {
        glow_line(&clip, pts(c.result), ACCENT, 2.2);
    }

    // Hover readout.
    if let Some(pos) = resp.hover_pos().filter(|p| plot.contains(*p)) {
        let f = 20.0 * 2f64.powf(((pos.x - plot.left()) / plot.width()) as f64 * span);
        let i = c.grid.partition_point(|&g| g < f).min(c.grid.len() - 1);
        let x = fx(c.grid[i]);
        clip.line_segment([Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())], Stroke::new(1.0, alpha(TEXT, 0.25)));
        let mut rows: Vec<(&str, Color32, f32)> = Vec::new();
        if layers.target {
            rows.push(("Target", TEXT, c.target[i]));
        }
        if layers.measured {
            rows.push(("Headphone", AMBER, c.measured[i]));
        }
        if layers.eq {
            rows.push(("Correction", VIOLET, c.eq[i]));
        }
        if layers.result {
            rows.push(("Result", ACCENT, c.result[i]));
        }
        for (_, col, v) in &rows {
            clip.circle_filled(Pos2::new(x, fy(*v)), 3.5, *col);
        }
        let w = 168.0;
        let h = 26.0 + rows.len() as f32 * 17.0;
        let mut bx = pos.x + 14.0;
        if bx + w > plot.right() {
            bx = pos.x - 14.0 - w;
        }
        let by = (pos.y - h / 2.0).clamp(plot.top() + 4.0, plot.bottom() - h - 4.0);
        let r = Rect::from_min_size(Pos2::new(bx, by), Vec2::new(w, h));
        painter.rect_filled(r, CornerRadius::same(8), alpha(CARD_HI, 0.96));
        painter.rect_stroke(r, CornerRadius::same(8), Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
        painter.text(
            r.left_top() + Vec2::new(10.0, 7.0),
            Align2::LEFT_TOP,
            format!("{} Hz", fmt_freq(c.grid[i])),
            mono(12.0),
            TEXT,
        );
        for (k, (name, col, v)) in rows.iter().enumerate() {
            let y = r.top() + 27.0 + k as f32 * 17.0;
            painter.circle_filled(Pos2::new(r.left() + 13.0, y + 6.0), 3.0, *col);
            painter.text(Pos2::new(r.left() + 22.0, y), Align2::LEFT_TOP, *name, prop(11.5), DIM);
            painter.text(Pos2::new(r.right() - 10.0, y), Align2::RIGHT_TOP, format!("{v:+.1} dB"), mono(11.5), *col);
        }
    }
}
