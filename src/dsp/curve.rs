//! Frequency-response curves on a log-frequency grid.

use anyhow::{Result, bail};

/// Grid resolution used everywhere (points per octave).
pub const PER_OCT: f64 = 48.0;
/// Lowest frequency of the grid, in Hz.
pub const F_MIN: f64 = 20.0;
/// Highest frequency of the grid, in Hz.
pub const F_MAX: f64 = 20_000.0;

/// 1/48-octave grid from 20 Hz to just above 20 kHz (~480 points).
pub fn log_grid() -> Vec<f64> {
    let n = ((F_MAX / F_MIN).log2() * PER_OCT).ceil() as usize + 1;
    (0..n).map(|i| F_MIN * 2f64.powf(i as f64 / PER_OCT)).collect()
}

/// A measured or target curve: frequency (Hz) → level (dB).
#[derive(Clone, Debug)]
pub struct Curve {
    /// Frequencies in Hz, strictly increasing.
    pub freq: Vec<f64>,
    /// Levels in dB, one per frequency.
    pub db: Vec<f64>,
}

impl Curve {
    /// Parses AutoEq / REW / generic CSV-ish text: first two numeric columns
    /// are frequency and dB. Header / comment lines are skipped.
    pub fn parse(text: &str) -> Result<Self> {
        let mut pts: Vec<(f64, f64)> = Vec::new();
        for line in text.lines() {
            let mut nums = line
                .split(|c: char| c == ',' || c == ';' || c == '\t' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<f64>());
            if let (Some(Ok(f)), Some(Ok(db))) = (nums.next(), nums.next())
                && f > 0.0
                && f.is_finite()
                && db.is_finite()
            {
                pts.push((f, db));
            }
        }
        if pts.len() < 10 {
            bail!("not a frequency response (need at least 10 'frequency, dB' rows)");
        }
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        pts.dedup_by(|a, b| a.0 == b.0);
        let (freq, db) = pts.into_iter().unzip();
        Ok(Curve { freq, db })
    }

    /// Value at `f`, linearly interpolated in log-frequency, clamped at the ends.
    pub fn at(&self, f: f64) -> f64 {
        let n = self.freq.len();
        if f <= self.freq[0] {
            return self.db[0];
        }
        if f >= self.freq[n - 1] {
            return self.db[n - 1];
        }
        let i = self.freq.partition_point(|&x| x <= f); // freq[i-1] <= f < freq[i]
        let (f0, f1) = (self.freq[i - 1].ln(), self.freq[i].ln());
        let t = (f.ln() - f0) / (f1 - f0);
        self.db[i - 1] + t * (self.db[i] - self.db[i - 1])
    }

    /// Samples the curve on `grid` (see [`Curve::at`]).
    pub fn sample(&self, grid: &[f64]) -> Vec<f64> {
        grid.iter().map(|&f| self.at(f)).collect()
    }
}

/// Mean of `values` for grid frequencies inside `[lo, hi]`.
pub fn band_mean(grid: &[f64], values: &[f64], lo: f64, hi: f64) -> f64 {
    let (mut s, mut n) = (0.0, 0usize);
    for (&f, &v) in grid.iter().zip(values) {
        if f >= lo && f <= hi {
            s += v;
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { s / n as f64 }
}

/// Fractional-octave smoothing with a frequency-dependent window
/// (`window_oct(f)` = full window width in octaves). Grid must be `log_grid()`.
pub fn smooth(grid: &[f64], values: &[f64], window_oct: impl Fn(f64) -> f64) -> Vec<f64> {
    let n = values.len();
    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.0);
    for v in values {
        prefix.push(prefix.last().unwrap() + v);
    }
    (0..n)
        .map(|i| {
            let half = ((window_oct(grid[i]) * PER_OCT) / 2.0).round() as usize;
            // Symmetric window shrunk near the edges so the ends are not biased.
            let h = half.min(i).min(n - 1 - i);
            let (a, b) = (i - h, i + h + 1);
            (prefix[b] - prefix[a]) / (b - a) as f64
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_spans_audio_band() {
        let g = log_grid();
        assert!((g[0] - 20.0).abs() < 1e-9);
        assert!(*g.last().unwrap() >= 20_000.0 && *g.last().unwrap() < 20_400.0);
        assert!(g.len() > 470 && g.len() < 490);
    }

    #[test]
    fn parses_autoeq_csv_and_interpolates() {
        let c = Curve::parse("frequency,raw\n20,1\n40,3\n80,5\n100,5\n200,5\n400,5\n800,5\n1000,5\n2000,5\n4000,5\n")
            .unwrap();
        assert!((c.at(20.0) - 1.0).abs() < 1e-9);
        assert!((c.at(28.284271) - 2.0).abs() < 1e-3); // geometric midpoint
        assert!((c.at(10.0) - 1.0).abs() < 1e-9); // clamped
    }

    #[test]
    fn smoothing_keeps_constant_curves_constant() {
        let g = log_grid();
        let v = vec![3.0; g.len()];
        let s = smooth(&g, &v, |_| 1.0 / 3.0);
        assert!(s.iter().all(|x| (x - 3.0).abs() < 1e-12));
    }
}
