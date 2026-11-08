//! The "aura": the living light behind the interface.
//!
//! A few very large, soft radial gradients (pulse, halo, sun, depth) drift
//! slowly; their intensity follows the output level, and they dim and lose
//! colour when the correction is bypassed or paused.

use super::theme::*;
use eframe::egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Vec2};
use std::f32::consts::TAU;

/// Smoothed state of the light.
pub struct Aura {
    energy: f32,
    sat: f32,
    opacity: f32,
}

impl Default for Aura {
    fn default() -> Self {
        Self { energy: 0.0, sat: 1.0, opacity: 1.0 }
    }
}

/// How the light should look.
#[derive(Clone, Copy, PartialEq)]
pub enum Mood {
    /// Correction on: full colour.
    Live,
    /// Bypass: dim and desaturated.
    Bypass,
    /// Paused, disconnected, blocked: almost off.
    Paused,
}

fn desaturate(c: Color32, sat: f32) -> Color32 {
    let l = 0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32;
    let mix = |x: u8| (l + (x as f32 - l) * sat).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
}

/// Soft radial blob: `color` at the centre fading to transparent at `radius`.
fn blob(painter: &Painter, center: Pos2, radius: f32, color: Color32, strength: f32) {
    const SEG: usize = 72;
    // Gaussian-like falloff over a few rings.
    let rings = [(0.0, 1.0), (0.22, 0.85), (0.45, 0.52), (0.68, 0.22), (0.85, 0.07), (1.0, 0.0)];
    let mut m = Mesh::default();
    for &(r, a) in &rings {
        for s in 0..SEG {
            let ang = s as f32 / SEG as f32 * TAU;
            let p = center + Vec2::new(ang.cos(), ang.sin()) * (r * radius).max(0.01);
            m.colored_vertex(p, alpha(color, a * strength));
        }
    }
    for ring in 0..rings.len() - 1 {
        let (a0, a1) = ((ring * SEG) as u32, ((ring + 1) * SEG) as u32);
        for s in 0..SEG as u32 {
            let n = (s + 1) % SEG as u32;
            m.add_triangle(a0 + s, a0 + n, a1 + s);
            m.add_triangle(a0 + n, a1 + n, a1 + s);
        }
    }
    painter.add(Shape::mesh(m));
}

impl Aura {
    /// Eases toward the target mood; `level` is the output peak (0 to 1).
    pub fn update(&mut self, mood: Mood, level: f32, dt: f32) {
        let (sat, op) = match mood {
            Mood::Live => (1.0, 1.0),
            Mood::Bypass => (0.25, 0.38),
            Mood::Paused => (0.15, 0.22),
        };
        let k = 1.0 - (-dt / 0.35).exp();
        self.sat += (sat - self.sat) * k;
        self.opacity += (op - self.opacity) * k;
        // Perceptual energy from the peak, fast attack, slow release.
        let e = ((20.0 * level.max(1e-5).log10() + 50.0) / 50.0).clamp(0.0, 1.0);
        let ke = if e > self.energy { 1.0 - (-dt / 0.05).exp() } else { 1.0 - (-dt / 0.8).exp() };
        self.energy += (e - self.energy) * ke;
    }

    /// True while still easing (the caller keeps repainting).
    pub fn settling(&self, mood: Mood) -> bool {
        let target = match mood {
            Mood::Live => 1.0,
            Mood::Bypass => 0.38,
            Mood::Paused => 0.22,
        };
        (self.opacity - target).abs() > 0.01
    }

    /// Paints the light over `rect` (call before any content).
    pub fn paint(&self, painter: &Painter, rect: Rect, time: f64) {
        painter.rect_filled(rect, 0.0, NIGHT);
        let t = time as f32;
        let (w, h) = (rect.width(), rect.height());
        let at = |fx: f32, fy: f32| rect.min + Vec2::new(fx * w, fy * h);
        let drift = |speed: f32, phase: f32, amp: f32| {
            Vec2::new((t * speed + phase).sin(), (t * speed * 0.77 + phase * 1.3).cos()) * amp
        };
        let e = self.energy;
        let o = self.opacity;
        let c = |col: Color32| desaturate(col, self.sat);

        blob(
            painter,
            at(0.60, 0.02) + drift(0.050, 0.0, 70.0),
            w * 0.55 * (1.0 + 0.08 * e),
            c(PULSE),
            o * (0.62 + 0.30 * e),
        );
        blob(
            painter,
            at(0.86, 0.42) + drift(0.041, 2.1, 90.0),
            w * 0.40 * (1.0 + 0.06 * e),
            c(HALO),
            o * (0.42 + 0.20 * e),
        );
        blob(painter, at(0.66, 0.06) + drift(0.063, 4.0, 50.0), w * 0.18, c(SOLEIL), o * (0.30 + 0.25 * e));
        blob(painter, at(0.04, 0.96) + drift(0.035, 1.0, 60.0), w * 0.45, c(DEPTH), o * 0.9);

        // Vignette toward the bottom so the cards sit on calm ground.
        let mut m = Mesh::default();
        let top = rect.top() + h * 0.40;
        m.colored_vertex(Pos2::new(rect.left(), top), Color32::TRANSPARENT);
        m.colored_vertex(Pos2::new(rect.right(), top), Color32::TRANSPARENT);
        m.colored_vertex(rect.right_bottom(), alpha(NIGHT, 0.88));
        m.colored_vertex(rect.left_bottom(), alpha(NIGHT, 0.88));
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        painter.add(Shape::mesh(m));
    }
}
