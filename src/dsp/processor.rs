//! Real-time stereo processor. No allocation, no locks, no syscalls:
//! safe to run inside the CoreAudio IO callback.

use super::biquad::{Coeffs, Filter, State};

/// Maximum number of EQ bands the real-time processor runs.
pub const MAX_FILTERS: usize = 16;
/// Crossfade length when the EQ changes (avoids zipper noise / clicks).
const XFADE_SAMPLES: usize = 1024;

/// Everything the audio thread needs for one EQ setting (plain `Copy` data,
/// sent through a lock-free ring buffer).
#[derive(Clone, Copy, Debug)]
pub struct EqSettings {
    /// Coefficients of the active bands (the first [`EqSettings::count`]).
    pub coeffs: [Coeffs; MAX_FILTERS],
    /// Number of active bands.
    pub count: usize,
    /// Linear gain applied to the corrected signal (negative preamp → headroom).
    pub preamp: f64,
    /// Linear gain applied to the bypassed signal (loudness-matched A/B).
    pub dry_gain: f64,
}

impl EqSettings {
    /// Settings that leave the signal untouched.
    pub fn identity() -> Self {
        Self { coeffs: [Coeffs::IDENTITY; MAX_FILTERS], count: 0, preamp: 1.0, dry_gain: 1.0 }
    }

    /// Designs the coefficients of `filters` at `fs`, with a preamp and a
    /// bypass (dry) gain in dB.
    pub fn from_filters(filters: &[Filter], preamp_db: f64, dry_gain_db: f64, fs: f64) -> Self {
        let mut s = Self::identity();
        for (slot, f) in s.coeffs.iter_mut().zip(filters.iter().take(MAX_FILTERS)) {
            *slot = Coeffs::design(f, fs);
        }
        s.count = filters.len().min(MAX_FILTERS);
        s.preamp = 10f64.powf(preamp_db / 20.0);
        s.dry_gain = 10f64.powf(dry_gain_db / 20.0);
        s
    }
}

#[derive(Clone, Copy)]
struct Chain {
    eq: EqSettings,
    st: [[State; 2]; MAX_FILTERS],
}

impl Chain {
    fn new(eq: EqSettings) -> Self {
        Self { eq, st: [[State::default(); 2]; MAX_FILTERS] }
    }

    /// Returns (wet L, wet R).
    #[inline(always)]
    fn tick(&mut self, l: f64, r: f64) -> (f64, f64) {
        let (mut l, mut r) = (l, r);
        for i in 0..self.eq.count {
            let c = &self.eq.coeffs[i];
            l = self.st[i][0].tick(c, l);
            r = self.st[i][1].tick(c, r);
        }
        (l * self.eq.preamp, r * self.eq.preamp)
    }

    fn flush(&mut self) {
        for s in self.st.iter_mut().flatten() {
            s.flush();
        }
    }
}

/// The stereo correction processor run inside the audio callback.
///
/// Two filter chains allow a click-free crossfade when the EQ changes, and a
/// smoothed wet/dry ramp handles bypass.
pub struct Processor {
    cur: Chain,
    next: Chain,
    xfade_left: usize,
    /// 0 = bypassed, 1 = fully corrected. Smoothed toward `wet_target`.
    wet: f64,
    wet_target: f64,
    wet_coef: f64,
}

impl Processor {
    /// Creates a processor running `eq` at sample rate `fs`.
    pub fn new(eq: EqSettings, fs: f64) -> Self {
        let c = Chain::new(eq);
        Self {
            cur: c,
            next: c,
            xfade_left: 0,
            wet: 1.0,
            wet_target: 1.0,
            // ~8 ms one-pole for bypass transitions.
            wet_coef: 1.0 - (-1.0 / (0.008 * fs)).exp(),
        }
    }

    /// Install new EQ settings with a short crossfade.
    pub fn set_eq(&mut self, eq: EqSettings) {
        if self.xfade_left > 0 {
            self.cur = self.next; // finish the pending fade instantly
        }
        self.next = Chain { eq, st: self.cur.st };
        self.xfade_left = XFADE_SAMPLES;
    }

    /// Bypasses (or re-enables) the correction, with an ~8 ms ramp.
    pub fn set_bypass(&mut self, bypass: bool) {
        self.wet_target = if bypass { 0.0 } else { 1.0 };
    }

    /// Process interleaved stereo frames in place.
    pub fn process(&mut self, buf: &mut [[f32; 2]]) {
        for fr in buf.iter_mut() {
            let (x_l, x_r) = (fr[0] as f64, fr[1] as f64);
            self.wet += (self.wet_target - self.wet) * self.wet_coef;
            let w = self.wet;

            let (wl, wr) = self.cur.tick(x_l, x_r);
            let dg = self.cur.eq.dry_gain;
            let mut out_l = x_l * dg * (1.0 - w) + wl * w;
            let mut out_r = x_r * dg * (1.0 - w) + wr * w;

            if self.xfade_left > 0 {
                let (nl, nr) = self.next.tick(x_l, x_r);
                let ndg = self.next.eq.dry_gain;
                let nl = x_l * ndg * (1.0 - w) + nl * w;
                let nr = x_r * ndg * (1.0 - w) + nr * w;
                let t = 1.0 - self.xfade_left as f64 / XFADE_SAMPLES as f64;
                out_l += (nl - out_l) * t;
                out_r += (nr - out_r) * t;
                self.xfade_left -= 1;
                if self.xfade_left == 0 {
                    self.cur = self.next;
                }
            }
            fr[0] = out_l as f32;
            fr[1] = out_r as f32;
        }
        self.cur.flush();
        self.next.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::biquad::FilterKind;

    #[test]
    fn identity_is_bit_transparent_and_zero_latency() {
        let mut p = Processor::new(EqSettings::identity(), 48_000.0);
        let mut buf = [[0.0f32; 2]; 64];
        buf[0] = [1.0, -0.5];
        p.process(&mut buf);
        assert_eq!(buf[0], [1.0, -0.5], "impulse must come out on sample 0");
        assert!(buf[1..].iter().all(|f| f == &[0.0, 0.0]));
    }

    #[test]
    fn eq_impulse_starts_at_sample_zero() {
        // Minimum-phase IIR: the response begins immediately (no pre-ringing, no delay).
        let f = [Filter { kind: FilterKind::Peak, freq: 1000.0, q: 1.0, gain_db: -6.0 }];
        let mut p = Processor::new(EqSettings::from_filters(&f, 0.0, 0.0, 48_000.0), 48_000.0);
        let mut buf = [[0.0f32; 2]; 32];
        buf[0] = [1.0, 1.0];
        p.process(&mut buf);
        let peak = buf.iter().enumerate().max_by(|a, b| a.1[0].abs().total_cmp(&b.1[0].abs())).unwrap().0;
        assert_eq!(peak, 0);
    }

    #[test]
    fn bypass_ramps_to_dry() {
        let f = [Filter { kind: FilterKind::Peak, freq: 1000.0, q: 1.0, gain_db: -12.0 }];
        let mut p = Processor::new(EqSettings::from_filters(&f, 0.0, 0.0, 48_000.0), 48_000.0);
        p.set_bypass(true);
        let mut buf = vec![[0.25f32, 0.25]; 48_000];
        p.process(&mut buf);
        assert!((buf.last().unwrap()[0] - 0.25).abs() < 1e-4);
    }
}
