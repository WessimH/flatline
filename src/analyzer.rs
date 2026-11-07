//! Real-time analyzer (RTA): FFT of the pre/post signals, integrated into
//! 1/6-octave bands so pink noise / music reads flat. Runs on the UI thread.

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};
use std::sync::Arc;

/// FFT size (≈170 ms at 48 kHz, 5.9 Hz bins).
pub const FFT_N: usize = 8192;
const POINTS_PER_OCT: f64 = 24.0;
const BAND_OCT: f64 = 1.0 / 6.0;
/// Level shown for silence, in dBFS.
pub const FLOOR_DB: f32 = -110.0;

/// Real-time analyzer for the signal before (`db[0]`) and after (`db[1]`) correction.
pub struct Analyzer {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    ring: [Vec<f32>; 2],
    pos: usize,
    input: Vec<f32>,
    spec: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    power: Vec<f32>,
    bands: Vec<(f64, f64)>, // fractional bin range per display point
    /// Display frequencies (1/24-octave grid, 20 Hz to 20 kHz).
    pub freqs: Vec<f64>,
    /// [pre, post] levels in dBFS (smoothed), one per `freqs`.
    pub db: [Vec<f32>; 2],
    fs: f64,
}

impl Analyzer {
    /// Creates an analyzer for sample rate `fs`.
    pub fn new(fs: f64) -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_N);
        let spec = fft.make_output_vec();
        let scratch = fft.make_scratch_vec();
        let window =
            (0..FFT_N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_N as f32).cos()).collect();
        let n = ((20_000f64 / 20.0).log2() * POINTS_PER_OCT) as usize + 1;
        let freqs: Vec<f64> = (0..n).map(|i| 20.0 * 2f64.powf(i as f64 / POINTS_PER_OCT)).collect();
        let mut a = Self {
            fft,
            window,
            ring: [vec![0.0; FFT_N], vec![0.0; FFT_N]],
            pos: 0,
            input: vec![0.0; FFT_N],
            power: vec![0.0; spec.len()],
            spec,
            scratch,
            bands: Vec::new(),
            db: [vec![FLOOR_DB; freqs.len()], vec![FLOOR_DB; freqs.len()]],
            freqs,
            fs: 0.0,
        };
        a.set_rate(fs);
        a
    }

    /// Updates the band mapping for a new sample rate (no-op if unchanged).
    pub fn set_rate(&mut self, fs: f64) {
        if (fs - self.fs).abs() < 1.0 {
            return;
        }
        self.fs = fs;
        let bin = fs / FFT_N as f64;
        let h = 2f64.powf(BAND_OCT / 2.0);
        self.bands = self.freqs.iter().map(|&f| ((f / h) / bin, (f * h) / bin)).collect();
    }

    #[inline]
    /// Pushes one `[pre, post]` mono sample pair into the ring buffer.
    pub fn push(&mut self, s: [f32; 2]) {
        self.ring[0][self.pos] = s[0];
        self.ring[1][self.pos] = s[1];
        self.pos = (self.pos + 1) % FFT_N;
    }

    /// Runs the FFTs on the latest [`FFT_N`] samples and updates [`Analyzer::db`] with RTA ballistics.
    pub fn compute(&mut self) {
        // Full-scale sine → 0 dBFS (Hann window, one-sided spectrum).
        let norm = 32.0 / (3.0 * (FFT_N as f32).powi(2));
        for ch in 0..2 {
            for i in 0..FFT_N {
                let j = (self.pos + i) % FFT_N;
                self.input[i] = self.ring[ch][j] * self.window[i];
            }
            if self.fft.process_with_scratch(&mut self.input, &mut self.spec, &mut self.scratch).is_err() {
                return;
            }
            for (p, c) in self.power.iter_mut().zip(&self.spec) {
                *p = c.norm_sqr() * norm;
            }
            let nbins = self.power.len();
            for (k, &(lo, hi)) in self.bands.iter().enumerate() {
                // Integrate power over [lo, hi] with fractional bin overlap.
                let mut acc = 0.0f64;
                let first = (lo - 0.5).floor().max(0.0) as usize;
                let last = ((hi + 0.5).ceil() as usize).min(nbins - 1);
                for b in first..=last {
                    let (b0, b1) = (b as f64 - 0.5, b as f64 + 0.5);
                    let ov = (hi.min(b1) - lo.max(b0)).max(0.0);
                    acc += self.power[b] as f64 * ov;
                }
                let v = (10.0 * acc.max(1e-14).log10()) as f32;
                let v = v.max(FLOOR_DB);
                let cur = &mut self.db[ch][k];
                // Fast attack, slow release (~ RTA ballistics).
                let coef = if v > *cur { 0.55 } else { 0.10 };
                *cur += (v - *cur) * coef;
            }
        }
    }

    /// Decay toward silence (used when no audio is flowing).
    pub fn decay(&mut self) {
        for ch in &mut self.db {
            for v in ch.iter_mut() {
                *v += (FLOOR_DB - *v) * 0.08;
            }
        }
    }
}
