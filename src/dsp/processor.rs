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
    /// EQ received during a fade; it starts when that fade ends (latest wins).
    pending: Option<EqSettings>,
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
            pending: None,
            wet: 1.0,
            wet_target: 1.0,
            // ~8 ms one-pole for bypass transitions.
            wet_coef: 1.0 - (-1.0 / (0.008 * fs)).exp(),
        }
    }

    /// Install new EQ settings with a short crossfade.
    ///
    /// During a fade, the new EQ waits for that fade to end: cutting the fade
    /// short would make the output jump (a click).
    pub fn set_eq(&mut self, eq: EqSettings) {
        if self.xfade_left > 0 {
            self.pending = Some(eq);
        } else {
            self.start_fade(eq);
        }
    }

    fn start_fade(&mut self, eq: EqSettings) {
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
            // A NaN or infinite input would stay in the filter state forever: mute it.
            let clean = |x: f32| if x.is_finite() { x as f64 } else { 0.0 };
            let (x_l, x_r) = (clean(fr[0]), clean(fr[1]));
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
                    if let Some(eq) = self.pending.take() {
                        self.start_fade(eq);
                    }
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

    const FS: f64 = 48_000.0;

    /// +12 dB (or `gain_db`) peak at 200 Hz, the frequency of [`sine`].
    fn boost(gain_db: f64) -> EqSettings {
        let f = [Filter { kind: FilterKind::Peak, freq: 200.0, q: 1.0, gain_db }];
        EqSettings::from_filters(&f, 0.0, 0.0, FS)
    }

    /// 200 Hz stereo sine of amplitude 0.5, starting at sample `from`.
    fn sine(from: usize, len: usize) -> Vec<[f32; 2]> {
        (from..from + len)
            .map(|n| {
                let v = (0.5 * (2.0 * std::f64::consts::PI * 200.0 * n as f64 / FS).sin()) as f32;
                [v, v]
            })
            .collect()
    }

    /// Largest jump between two neighbouring samples (left channel).
    fn max_step(buf: &[[f32; 2]]) -> f32 {
        buf.windows(2).map(|w| (w[1][0] - w[0][0]).abs()).fold(0.0, f32::max)
    }

    // A 200 Hz sine at amplitude 2.0 (0.5 boosted by 12 dB) moves by at most
    // 2 * 2π * 200 / 48000 ≈ 0.052 per sample. A click is a step well above that.
    const STEP_BOUND: f32 = 0.07;

    #[test]
    fn eq_change_mid_stream_has_no_click() {
        let mut p = Processor::new(EqSettings::identity(), FS);
        let mut out = sine(0, 700);
        p.process(&mut out);
        // Switch while the sine is far from zero: a hard switch would jump.
        p.set_eq(boost(12.0));
        let mut rest = sine(700, 4000);
        p.process(&mut rest);
        out.extend_from_slice(&rest);

        let mut hard = Processor::new(boost(12.0), FS);
        let mut hard_out = sine(0, 701);
        hard.process(&mut hard_out);
        assert!((hard_out[700][0] - out[699][0]).abs() > STEP_BOUND, "test must switch where a hard cut clicks");

        assert!(max_step(&out) < STEP_BOUND, "step {} >= {STEP_BOUND}", max_step(&out));
    }

    #[test]
    fn crossfade_ends_after_exactly_xfade_samples() {
        let mut p = Processor::new(EqSettings::identity(), FS);
        let new = boost(12.0);
        p.set_eq(new);
        let mut buf = sine(0, XFADE_SAMPLES - 1);
        p.process(&mut buf);
        assert_eq!(p.xfade_left, 1);
        assert_eq!(p.cur.eq.count, 0, "old EQ still current one sample before the end");

        p.process(&mut sine(XFADE_SAMPLES - 1, 1));
        assert_eq!(p.xfade_left, 0);
        assert_eq!(p.cur.eq.coeffs, new.coeffs);
        assert_eq!(p.cur.eq.count, new.count);
        assert_eq!(p.cur.eq.coeffs, p.next.eq.coeffs);
    }

    #[test]
    fn set_eq_during_fade_waits_for_the_fade_to_end() {
        let mut p = Processor::new(EqSettings::identity(), FS);
        let b = boost(12.0);
        let c = boost(-12.0);
        p.set_eq(b);
        p.process(&mut sine(0, 300));
        p.set_eq(boost(3.0));
        p.set_eq(c); // latest wins
        assert_eq!(p.next.eq.coeffs, b.coeffs, "the running fade goes on");
        assert_eq!(p.xfade_left, XFADE_SAMPLES - 300);

        p.process(&mut sine(300, XFADE_SAMPLES - 300));
        assert_eq!(p.cur.eq.coeffs, b.coeffs);
        assert_eq!(p.next.eq.coeffs, c.coeffs, "the waiting EQ starts its own fade");
        assert_eq!(p.xfade_left, XFADE_SAMPLES);
        assert!(p.pending.is_none());

        p.process(&mut sine(XFADE_SAMPLES, XFADE_SAMPLES));
        assert_eq!(p.cur.eq.coeffs, c.coeffs);
        assert_eq!(p.xfade_left, 0);
    }

    #[test]
    fn second_set_eq_during_fade_has_no_click() {
        let mut p = Processor::new(EqSettings::identity(), FS);
        let mut out = sine(0, 300);
        p.set_eq(boost(12.0));
        p.process(&mut out);
        p.set_eq(boost(-12.0));
        let mut rest = sine(300, 3000);
        p.process(&mut rest);
        out.extend_from_slice(&rest);
        assert!(max_step(&out) < STEP_BOUND, "step {} >= {STEP_BOUND}", max_step(&out));
    }

    #[test]
    fn same_eq_twice_leaves_output_unchanged() {
        let eq = boost(6.0);
        let mut steady = Processor::new(eq, FS);
        let mut faded = Processor::new(eq, FS);
        let mut a = sine(0, 3000);
        let mut b = a.clone();
        steady.process(&mut a[..500]);
        faded.process(&mut b[..500]);
        faded.set_eq(eq);
        steady.process(&mut a[500..]);
        faded.process(&mut b[500..]);
        assert_eq!(a, b);
    }

    #[test]
    fn extreme_gains_stay_finite() {
        for gain in [12.0, -12.0] {
            let mut p = Processor::new(boost(-gain), FS);
            p.set_eq(boost(gain));
            let mut buf = vec![[1.0f32, -1.0]; 4096];
            p.process(&mut buf);
            assert!(buf.iter().flatten().all(|v| v.is_finite()), "gain {gain} dB");
        }
    }

    #[test]
    fn silence_in_silence_out() {
        let mut p = Processor::new(boost(12.0), FS);
        p.set_eq(boost(-12.0));
        let mut buf = vec![[0.0f32; 2]; 4096];
        p.process(&mut buf);
        assert!(buf.iter().flatten().all(|&v| v == 0.0));
    }

    #[test]
    fn nan_or_inf_input_does_not_poison_next_block() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut p = Processor::new(boost(12.0), FS);
            p.set_eq(boost(-12.0)); // both chains run
            let mut buf = sine(0, 256);
            buf[10] = [bad, bad];
            p.process(&mut buf);
            assert!(buf.iter().flatten().all(|v| v.is_finite()), "input {bad} leaked into its own block");
            let mut next = sine(256, 256);
            p.process(&mut next);
            assert!(next.iter().flatten().all(|v| v.is_finite()), "input {bad} leaked into the next block");
        }
    }
}
