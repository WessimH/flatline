//! RBJ "Audio EQ Cookbook" biquads.
//!
//! Every filter here is a minimum-phase IIR: it adds **zero samples** of
//! latency. That is the whole point of the design (same idea as the
//! "zero latency" mode of commercial calibration tools).

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Shape of a parametric EQ band.
pub enum FilterKind {
    /// Bell (peaking) filter.
    Peak,
    /// Low shelf.
    LowShelf,
    /// High shelf.
    HighShelf,
}

impl FilterKind {
    /// Equalizer APO / AutoEq abbreviation (`PK`, `LSC`, `HSC`).
    pub fn short(self) -> &'static str {
        match self {
            FilterKind::Peak => "PK",
            FilterKind::LowShelf => "LSC",
            FilterKind::HighShelf => "HSC",
        }
    }
}

/// A parametric EQ band (sample-rate independent description).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    /// Filter shape.
    pub kind: FilterKind,
    /// Centre or corner frequency, in Hz.
    pub freq: f64,
    /// Quality factor (bandwidth for peaks, slope for shelves).
    pub q: f64,
    /// Gain at the centre / on the shelf, in dB.
    pub gain_db: f64,
}

/// Normalised biquad coefficients (a0 == 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coeffs {
    /// Feed-forward coefficient b0.
    pub b0: f64,
    /// Feed-forward coefficient b1.
    pub b1: f64,
    /// Feed-forward coefficient b2.
    pub b2: f64,
    /// Feedback coefficient a1.
    pub a1: f64,
    /// Feedback coefficient a2.
    pub a2: f64,
}

impl Coeffs {
    /// A filter that passes the signal unchanged.
    pub const IDENTITY: Coeffs = Coeffs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    /// Designs RBJ cookbook coefficients for `f` at sample rate `fs`.
    ///
    /// The frequency is clamped below 0.45 × `fs`, so a profile designed at 48 kHz
    /// stays stable at 44.1 kHz.
    pub fn design(f: &Filter, fs: f64) -> Coeffs {
        // Keep the centre frequency safely below Nyquist whatever the device rate.
        let freq = f.freq.clamp(5.0, fs * 0.45);
        let q = f.q.max(0.05);
        let a = 10f64.powf(f.gain_db / 40.0);
        let w0 = 2.0 * PI * freq / fs;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let sqa = a.sqrt();

        let (b0, b1, b2, a0, a1, a2) = match f.kind {
            FilterKind::Peak => {
                (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a)
            }
            FilterKind::LowShelf => (
                a * ((a + 1.0) - (a - 1.0) * cos + 2.0 * sqa * alpha),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - 2.0 * sqa * alpha),
                (a + 1.0) + (a - 1.0) * cos + 2.0 * sqa * alpha,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - 2.0 * sqa * alpha,
            ),
            FilterKind::HighShelf => (
                a * ((a + 1.0) + (a - 1.0) * cos + 2.0 * sqa * alpha),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - 2.0 * sqa * alpha),
                (a + 1.0) - (a - 1.0) * cos + 2.0 * sqa * alpha,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - 2.0 * sqa * alpha,
            ),
        };
        Coeffs { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    /// Magnitude response in dB at `freq`.
    pub fn magnitude_db(&self, freq: f64, fs: f64) -> f64 {
        let w = 2.0 * PI * freq / fs;
        let (c1, c2) = (w.cos(), (2.0 * w).cos());
        let Coeffs { b0, b1, b2, a1, a2 } = *self;
        let num = b0 * b0 + b1 * b1 + b2 * b2 + 2.0 * (b0 * b1 + b1 * b2) * c1 + 2.0 * b0 * b2 * c2;
        let den = 1.0 + a1 * a1 + a2 * a2 + 2.0 * (a1 + a1 * a2) * c1 + 2.0 * a2 * c2;
        10.0 * (num.max(1e-30) / den.max(1e-30)).log10()
    }
}

/// Summed dB response of a cascade of filters over a frequency grid.
pub fn cascade_response(filters: &[Filter], grid: &[f64], fs: f64) -> Vec<f64> {
    let mut out = vec![0.0; grid.len()];
    for f in filters {
        let c = Coeffs::design(f, fs);
        for (o, &fr) in out.iter_mut().zip(grid) {
            *o += c.magnitude_db(fr, fs);
        }
    }
    out
}

/// Transposed Direct Form II state for one channel of one biquad.
#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    /// First delay element.
    pub s1: f64,
    /// Second delay element.
    pub s2: f64,
}

impl State {
    #[inline(always)]
    /// Processes one sample (transposed direct form II).
    pub fn tick(&mut self, c: &Coeffs, x: f64) -> f64 {
        let y = c.b0 * x + self.s1;
        self.s1 = c.b1 * x - c.a1 * y + self.s2;
        self.s2 = c.b2 * x - c.a2 * y;
        y
    }

    /// Flush denormal-range values to zero (called once per block).
    #[inline(always)]
    pub fn flush(&mut self) {
        if self.s1.abs() < 1e-25 {
            self.s1 = 0.0;
        }
        if self.s2.abs() < 1e-25 {
            self.s2 = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measure_gain(c: &Coeffs, freq: f64, fs: f64) -> f64 {
        // Run a sine through the filter and compare RMS after settling.
        let mut st = State::default();
        let n = (fs * 2.0) as usize;
        let (mut e_in, mut e_out) = (0.0, 0.0);
        for i in 0..n {
            let x = (2.0 * PI * freq * i as f64 / fs).sin();
            let y = st.tick(c, x);
            if i > n / 2 {
                e_in += x * x;
                e_out += y * y;
            }
        }
        10.0 * (e_out / e_in).log10()
    }

    #[test]
    fn analytic_magnitude_matches_time_domain() {
        let fs = 48_000.0;
        for f in [
            Filter { kind: FilterKind::Peak, freq: 1000.0, q: 2.0, gain_db: 6.0 },
            Filter { kind: FilterKind::LowShelf, freq: 105.0, q: 0.7, gain_db: -4.0 },
            Filter { kind: FilterKind::HighShelf, freq: 10_000.0, q: 0.7, gain_db: -5.4 },
        ] {
            let c = Coeffs::design(&f, fs);
            for probe in [50.0, 105.0, 1000.0, 3000.0, 12_000.0] {
                let a = c.magnitude_db(probe, fs);
                let m = measure_gain(&c, probe, fs);
                assert!((a - m).abs() < 0.05, "{f:?} @ {probe}: analytic {a} vs measured {m}");
            }
        }
    }

    #[test]
    fn peak_hits_its_gain_at_centre() {
        let fs = 48_000.0;
        let f = Filter { kind: FilterKind::Peak, freq: 3684.0, q: 2.88, gain_db: 6.8 };
        let g = Coeffs::design(&f, fs).magnitude_db(3684.0, fs);
        assert!((g - 6.8).abs() < 0.01, "{g}");
    }
}
