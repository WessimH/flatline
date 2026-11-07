//! Correction design: build the target curve, derive the wanted correction
//! from the headphone measurement, then fit a small set of parametric EQ
//! bands to it (greedy seeding + Levenberg–Marquardt refinement).

use super::biquad::{Coeffs, Filter, FilterKind, cascade_response};
use super::curve::{Curve, F_MAX, F_MIN, band_mean, log_grid, smooth};
use serde::{Deserialize, Serialize};

/// User-facing knobs of the correction.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DesignParams {
    /// Low-shelf added to the target (105 Hz, Q 0.7). Harman 2018 ≈ +6.5 dB.
    pub bass_db: f64,
    /// Spectral tilt added to the target, pivoting at 1 kHz.
    pub tilt_db_oct: f64,
    /// Never boost more than this anywhere.
    pub max_boost_db: f64,
    /// Number of EQ bands (2 shelves + peaks).
    pub n_filters: usize,
}

impl Default for DesignParams {
    fn default() -> Self {
        Self { bass_db: 6.5, tilt_db_oct: 0.0, max_boost_db: 8.0, n_filters: 10 }
    }
}

/// Result of a design, everything sampled on `grid`.
#[derive(Clone, Debug)]
pub struct Design {
    /// Frequencies of every curve below (see [`crate::dsp::curve::log_grid`]).
    pub grid: Vec<f64>,
    /// Headphone response, level-aligned to the target.
    pub measured: Vec<f64>,
    /// Target response including the bass shelf and tilt.
    pub target: Vec<f64>,
    /// Fitted bands (100 % amount).
    pub filters: Vec<Filter>,
    /// RMS deviation from target (20 Hz–10 kHz) before / after correction.
    pub error_before_db: f64,
    /// RMS deviation from the target after correction, in dB.
    pub error_after_db: f64,
    /// Sample rate the filters were fitted at.
    pub fs: f64,
}

const BASS_FC: f64 = 105.0;
const BASS_Q: f64 = 0.7;
const ALIGN: (f64, f64) = (300.0, 3000.0);
const FIT_HI: f64 = 16_000.0;
const SHAPE_FS: f64 = 192_000.0; // target shapes are acoustic, ~analog

/// Builds the target curve: `base` plus a 105 Hz low shelf of `bass_db` and a
/// tilt of `tilt_db_oct` pivoting at 1 kHz.
pub fn build_target(base: &Curve, grid: &[f64], bass_db: f64, tilt_db_oct: f64) -> Vec<f64> {
    let shelf =
        Coeffs::design(&Filter { kind: FilterKind::LowShelf, freq: BASS_FC, q: BASS_Q, gain_db: bass_db }, SHAPE_FS);
    grid.iter().map(|&f| base.at(f) + shelf.magnitude_db(f, SHAPE_FS) + tilt_db_oct * (f / 1000.0).log2()).collect()
}

/// Fit weight: full up to 8 kHz, fading to 0.4 at 16 kHz, ignored above
/// (headphone treble above ~10 kHz depends heavily on fit / ear shape).
fn weight(f: f64) -> f64 {
    if f > FIT_HI {
        0.0
    } else if f > 8000.0 {
        1.0 - 0.6 * (f / 8000.0).log2()
    } else {
        1.0
    }
}

/// Smoothing window of the wanted correction: fine in the low/mid range,
/// broad in the treble so we only fix tonal balance, not ear-dependent notches.
fn smoothing_window(f: f64) -> f64 {
    const LO: f64 = 1.0 / 12.0;
    const HI: f64 = 1.0;
    if f <= 6000.0 {
        LO
    } else if f >= 8000.0 {
        HI
    } else {
        let t = (f / 6000.0).log2() / (8000.0f64 / 6000.0).log2();
        LO + t * (HI - LO)
    }
}

/// Designs the correction for a headphone `measured` response.
///
/// 1. aligns the measurement to the target between 300 Hz and 3 kHz;
/// 2. smooths `target − measured` (1/12 octave, widening to 1 octave above 6 kHz);
/// 3. caps boosts at `max_boost_db`;
/// 4. fits `n_filters` bands with [`fit`].
///
/// ```
/// use flatline::{dsp::fit, profiles};
///
/// let hp = profiles::HEADPHONES[0].curve();
/// let target = profiles::TARGETS[0].curve();
/// let d = fit::design(&hp, &target, &fit::DesignParams::default(), 48_000.0);
/// assert!(d.error_after_db < d.error_before_db);
/// ```
pub fn design(measured: &Curve, target_base: &Curve, p: &DesignParams, fs: f64) -> Design {
    let grid = log_grid();
    let target = build_target(target_base, &grid, p.bass_db, p.tilt_db_oct);
    let raw = measured.sample(&grid);
    let offset = band_mean(&grid, &target, ALIGN.0, ALIGN.1) - band_mean(&grid, &raw, ALIGN.0, ALIGN.1);
    let measured: Vec<f64> = raw.iter().map(|v| v + offset).collect();

    let wanted: Vec<f64> = target.iter().zip(&measured).map(|(t, m)| t - m).collect();
    let desired: Vec<f64> =
        smooth(&grid, &wanted, smoothing_window).into_iter().map(|v| v.min(p.max_boost_db)).collect();

    let filters = fit(&grid, &desired, p.n_filters.clamp(2, crate::dsp::processor::MAX_FILTERS), p.max_boost_db, fs);

    let eq = cascade_response(&filters, &grid, fs);
    let err = |corr: &dyn Fn(usize) -> f64| {
        let (mut s, mut n) = (0.0, 0.0);
        for (i, &f) in grid.iter().enumerate() {
            if f <= 10_000.0 {
                let e = measured[i] + corr(i) - target[i];
                s += e * e;
                n += 1.0;
            }
        }
        (s / n).sqrt()
    };
    // Level-independent: remove the mean offset before measuring the deviation.
    let mean_shift = |corr: &dyn Fn(usize) -> f64| {
        let v: Vec<f64> = (0..grid.len()).map(|i| measured[i] + corr(i) - target[i]).collect();
        band_mean(&grid, &v, 20.0, 10_000.0)
    };
    let b = mean_shift(&|_| 0.0);
    let a = mean_shift(&|i| eq[i]);
    let error_before_db = err(&|_| -b);
    let error_after_db = err(&|i| eq[i] - a);

    Design { grid, measured, target, filters, error_before_db, error_after_db, fs }
}

impl Design {
    /// Bands scaled by the dry/wet "amount" (0..1): gains scale linearly in dB,
    /// which keeps the correction minimum-phase (no comb filtering like a
    /// parallel dry/wet mix would create).
    pub fn scaled_filters(&self, amount: f64) -> Vec<Filter> {
        self.filters.iter().map(|f| Filter { gain_db: f.gain_db * amount, ..*f }).collect()
    }
}

/// Highest gain of the cascade anywhere in 20 Hz–20 kHz (dense 1/96 oct scan).
pub fn max_gain_db(filters: &[Filter], fs: f64) -> f64 {
    let n = ((F_MAX / F_MIN).log2() * 96.0) as usize;
    let grid: Vec<f64> = (0..=n).map(|i| F_MIN * 2f64.powf(i as f64 / 96.0)).collect();
    cascade_response(filters, &grid, fs).into_iter().fold(f64::MIN, f64::max)
}

// ---------------------------------------------------------------- fitting --

struct Bounds {
    logf: (f64, f64),
    lnq: (f64, f64),
    gain: (f64, f64),
}

fn bounds(kind: FilterKind) -> Bounds {
    let (f, q, g) = match kind {
        FilterKind::Peak => ((25.0, FIT_HI), (0.3, 7.0), (-18.0, 12.0)),
        FilterKind::LowShelf => ((40.0, 300.0), (0.4, 1.0), (-15.0, 15.0)),
        FilterKind::HighShelf => ((2000.0, 14_000.0), (0.4, 1.0), (-15.0, 12.0)),
    };
    Bounds { logf: (f64::log2(f.0), f64::log2(f.1)), lnq: (f64::ln(q.0), f64::ln(q.1)), gain: g }
}

fn to_filter(kind: FilterKind, p: &[f64]) -> Filter {
    Filter { kind, freq: 2f64.powf(p[0]), q: p[1].exp(), gain_db: p[2] }
}

fn clamp_params(kinds: &[FilterKind], p: &mut [f64]) {
    for (k, &kind) in kinds.iter().enumerate() {
        let b = bounds(kind);
        p[3 * k] = p[3 * k].clamp(b.logf.0, b.logf.1);
        p[3 * k + 1] = p[3 * k + 1].clamp(b.lnq.0, b.lnq.1);
        p[3 * k + 2] = p[3 * k + 2].clamp(b.gain.0, b.gain.1);
    }
}

fn filter_curve(f: &Filter, grid: &[f64], fs: f64) -> Vec<f64> {
    let c = Coeffs::design(f, fs);
    grid.iter().map(|&x| c.magnitude_db(x, fs)).collect()
}

/// Q of a peak whose half-gain width is `bw_oct` octaves.
fn q_from_bw(bw_oct: f64) -> f64 {
    let t = 2f64.powf(bw_oct);
    t.sqrt() / (t - 1.0)
}

/// Fits `n_filters` bands (a low shelf, a high shelf and peaks) to `desired`.
///
/// Greedy seeding on the largest residual, then a joint Levenberg–Marquardt
/// refinement of every frequency, Q and gain. Returns the bands sorted by
/// frequency.
pub fn fit(grid: &[f64], desired: &[f64], n_filters: usize, max_boost: f64, fs: f64) -> Vec<Filter> {
    // Points that matter for the fit.
    let idx: Vec<usize> = (0..grid.len()).filter(|&i| weight(grid[i]) > 0.0).collect();
    let g: Vec<f64> = idx.iter().map(|&i| grid[i]).collect();
    let d: Vec<f64> = idx.iter().map(|&i| desired[i]).collect();
    let w: Vec<f64> = g.iter().map(|&f| weight(f)).collect();

    // --- Seed: two shelves for the broad balance, then greedy peaks.
    let mut filters = vec![
        Filter {
            kind: FilterKind::LowShelf,
            freq: BASS_FC,
            q: BASS_Q,
            gain_db: band_mean(&g, &d, 20.0, 60.0).clamp(-12.0, max_boost),
        },
        Filter {
            kind: FilterKind::HighShelf,
            freq: 10_000.0,
            q: BASS_Q,
            gain_db: band_mean(&g, &d, 10_000.0, FIT_HI).clamp(-12.0, max_boost.min(6.0)),
        },
    ];
    let mut residual: Vec<f64> = {
        let r = cascade_response(&filters, &g, fs);
        d.iter().zip(r).map(|(a, b)| a - b).collect()
    };
    for _ in 2..n_filters {
        let (Some(i), _) =
            residual.iter().zip(&w).enumerate().fold((None, 0.0), |acc, (i, (r, wt))| {
                if r.abs() * wt > acc.1 { (Some(i), r.abs() * wt) } else { acc }
            })
        else {
            break;
        };
        let g0 = residual[i];
        let inside = |j: usize| residual[j].signum() == g0.signum() && residual[j].abs() > g0.abs() / 2.0;
        let (mut lo, mut hi) = (i, i);
        while lo > 0 && inside(lo - 1) {
            lo -= 1;
        }
        while hi + 1 < residual.len() && inside(hi + 1) {
            hi += 1;
        }
        let bw = ((g[hi] / g[lo]).log2()).max(1.0 / 12.0);
        let f = Filter {
            kind: FilterKind::Peak,
            freq: g[i],
            q: q_from_bw(bw).clamp(0.3, 7.0),
            gain_db: g0.clamp(-18.0, 12.0),
        };
        for (r, v) in residual.iter_mut().zip(filter_curve(&f, &g, fs)) {
            *r -= v;
        }
        filters.push(f);
    }

    // --- Joint refinement.
    let kinds: Vec<FilterKind> = filters.iter().map(|f| f.kind).collect();
    let mut p: Vec<f64> = filters.iter().flat_map(|f| [f.freq.log2(), f.q.ln(), f.gain_db]).collect();
    clamp_params(&kinds, &mut p);
    levenberg_marquardt(&kinds, &mut p, &g, &d, &w, fs, 200);

    let mut out: Vec<Filter> = kinds.iter().enumerate().map(|(k, &kind)| to_filter(kind, &p[3 * k..])).collect();
    out.sort_by(|a, b| a.freq.total_cmp(&b.freq));
    out
}

fn levenberg_marquardt(kinds: &[FilterKind], p: &mut [f64], g: &[f64], d: &[f64], w: &[f64], fs: f64, max_iter: usize) {
    let nf = kinds.len();
    let np = p.len();
    let n = g.len();
    let curve_of = |k: usize, p: &[f64]| filter_curve(&to_filter(kinds[k], &p[3 * k..]), g, fs);
    let total_of = |per: &[Vec<f64>]| {
        let mut t = vec![0.0; n];
        for c in per {
            for (a, b) in t.iter_mut().zip(c) {
                *a += b;
            }
        }
        t
    };
    let cost_of = |t: &[f64]| t.iter().zip(d).zip(w).map(|((t, d), w)| (w * (t - d)).powi(2)).sum::<f64>();

    let mut per: Vec<Vec<f64>> = (0..nf).map(|k| curve_of(k, p)).collect();
    let mut total = total_of(&per);
    let mut cost = cost_of(&total);
    let mut lambda = 1e-3;
    let h = 1e-5;
    let mut jac = vec![0.0; n * np]; // column-major: jac[j * n + i]

    for _ in 0..max_iter {
        for j in 0..np {
            let k = j / 3;
            let mut q = p.to_vec();
            q[j] += h;
            let c = curve_of(k, &q);
            for i in 0..n {
                jac[j * n + i] = w[i] * (c[i] - per[k][i]) / h;
            }
        }
        let r: Vec<f64> = (0..n).map(|i| w[i] * (total[i] - d[i])).collect();
        let mut a = vec![0.0; np * np];
        let mut grad = vec![0.0; np];
        for x in 0..np {
            let cx = &jac[x * n..(x + 1) * n];
            grad[x] = cx.iter().zip(&r).map(|(a, b)| a * b).sum();
            for y in x..np {
                let cy = &jac[y * n..(y + 1) * n];
                let v: f64 = cx.iter().zip(cy).map(|(a, b)| a * b).sum();
                a[x * np + y] = v;
                a[y * np + x] = v;
            }
        }

        let mut improved = false;
        for _ in 0..12 {
            let mut m = a.clone();
            for x in 0..np {
                m[x * np + x] += lambda * a[x * np + x] + 1e-9;
            }
            let Some(step) = solve(m, grad.iter().map(|v| -v).collect(), np) else {
                lambda *= 4.0;
                continue;
            };
            let mut q: Vec<f64> = p.iter().zip(&step).map(|(a, b)| a + b).collect();
            clamp_params(kinds, &mut q);
            let per_q: Vec<Vec<f64>> = (0..nf).map(|k| curve_of(k, &q)).collect();
            let total_q = total_of(&per_q);
            let cost_q = cost_of(&total_q);
            if cost_q < cost {
                let rel = (cost - cost_q) / cost.max(1e-12);
                p.copy_from_slice(&q);
                per = per_q;
                total = total_q;
                cost = cost_q;
                lambda = (lambda / 3.0).max(1e-9);
                improved = rel > 1e-7;
                break;
            }
            lambda *= 4.0;
        }
        if !improved {
            break;
        }
    }
}

/// Dense Gaussian elimination with partial pivoting (row-major `a`, n×n).
fn solve(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Option<Vec<f64>> {
    for col in 0..n {
        let piv = (col..n).max_by(|&x, &y| a[x * n + col].abs().total_cmp(&a[y * n + col].abs()))?;
        if a[piv * n + col].abs() < 1e-14 {
            return None;
        }
        if piv != col {
            for k in 0..n {
                a.swap(piv * n + k, col * n + k);
            }
            b.swap(piv, col);
        }
        for row in col + 1..n {
            let f = a[row * n + col] / a[col * n + col];
            if f != 0.0 {
                for k in col..n {
                    a[row * n + k] -= f * a[col * n + k];
                }
                b[row] -= f * b[col];
            }
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|k| a[row * n + k] * x[k]).sum();
        x[row] = (b[row] - s) / a[row * n + row];
    }
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles;

    fn dt770() -> (Curve, Curve) {
        let hp = profiles::HEADPHONES[0].curve();
        let tg = profiles::TARGETS[0].curve();
        (hp, tg)
    }

    /// AutoEq's published 10-band EQ for oratory1990's DT 770 PRO measurement.
    fn autoeq_reference() -> Vec<Filter> {
        use FilterKind::*;
        [
            (LowShelf, 105.0, 0.70, -3.0),
            (Peak, 6430.0, 0.97, -4.2),
            (Peak, 3684.0, 2.88, 6.8),
            (Peak, 213.0, 2.46, 3.5),
            (Peak, 139.0, 4.92, -3.9),
            (HighShelf, 10_000.0, 0.70, -5.4),
            (Peak, 93.0, 3.00, 3.5),
            (Peak, 9561.0, 1.86, 2.3),
            (Peak, 45.0, 1.74, -1.3),
            (Peak, 119.0, 5.83, -2.1),
        ]
        .into_iter()
        .map(|(kind, freq, q, gain_db)| Filter { kind, freq, q, gain_db })
        .collect()
    }

    fn deviation(d: &Design, filters: &[Filter]) -> f64 {
        let eq = cascade_response(filters, &d.grid, d.fs);
        let v: Vec<f64> = (0..d.grid.len()).map(|i| d.measured[i] + eq[i] - d.target[i]).collect();
        let m = band_mean(&d.grid, &v, 20.0, 10_000.0);
        let (mut s, mut n) = (0.0, 0.0);
        for (i, &f) in d.grid.iter().enumerate() {
            if f <= 10_000.0 {
                s += (v[i] - m).powi(2);
                n += 1.0;
            }
        }
        (s / n).sqrt()
    }

    #[test]
    fn fit_is_at_least_as_good_as_autoeq_reference() {
        let (hp, tg) = dt770();
        let d = design(&hp, &tg, &DesignParams::default(), 48_000.0);
        let ours = deviation(&d, &d.filters);
        let theirs = deviation(&d, &autoeq_reference());
        eprintln!("before {:.2} dB | ours {:.2} dB | AutoEq {:.2} dB", d.error_before_db, ours, theirs);
        for f in &d.filters {
            eprintln!("  {:>3} {:>7.0} Hz  Q {:>4.2}  {:>+5.1} dB", f.kind.short(), f.freq, f.q, f.gain_db);
        }
        assert!(d.error_after_db < d.error_before_db * 0.5);
        assert!(ours <= theirs * 1.15, "ours {ours} vs AutoEq {theirs}");
    }

    #[test]
    fn never_boosts_past_limit_by_much() {
        let (hp, tg) = dt770();
        for boost in [3.0, 6.0, 10.0] {
            let p = DesignParams { max_boost_db: boost, ..Default::default() };
            let d = design(&hp, &tg, &p, 48_000.0);
            let m = max_gain_db(&d.filters, 48_000.0);
            assert!(m < boost + 2.0, "boost limit {boost}: max gain {m}");
        }
    }

    #[test]
    fn solver_solves() {
        let x = solve(vec![2.0, 1.0, 1.0, 3.0], vec![3.0, 5.0], 2).unwrap();
        assert!((x[0] - 0.8).abs() < 1e-12 && (x[1] - 1.4).abs() < 1e-12);
    }
}
