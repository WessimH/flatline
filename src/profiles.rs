//! Built-in measurements and targets.
//!
//! Headphone data: oratory1990 measurements (GRAS 43AG-7), via the AutoEq
//! project (github.com/jaakkopasanen/AutoEq). Targets come from the same repo.

use crate::dsp::curve::Curve;

/// A built-in headphone measurement.
pub struct Profile {
    /// Display name.
    pub name: &'static str,
    /// Source and context of the measurement.
    pub note: &'static str,
    csv: &'static str,
}

impl Profile {
    /// Parses the embedded measurement.
    pub fn curve(&self) -> Curve {
        Curve::parse(self.csv).expect("embedded curve is valid")
    }
}

/// Built-in headphone measurements.
pub static HEADPHONES: &[Profile] = &[
    Profile {
        name: "DT 770 PRO",
        note: "oratory1990 · stock velour pads (32 / 80 / 250 Ω share the same acoustics)",
        csv: include_str!("../data/headphones/Beyerdynamic DT 770 Pro.csv"),
    },
    Profile {
        name: "DT 770 PRO · worn pads",
        note: "oratory1990 · compressed / old velour pads (more bass, less seal)",
        csv: include_str!("../data/headphones/Beyerdynamic DT 770 Pro (worn earpads).csv"),
    },
    Profile {
        name: "DT 770 PRO · Dekoni Hybrid",
        note: "oratory1990 · Dekoni hybrid earpads",
        csv: include_str!("../data/headphones/Beyerdynamic DT 770 Pro (Dekoni hybrid earpads).csv"),
    },
    Profile {
        name: "DT 770 PRO X Limited",
        note: "oratory1990 · DT 770 PRO X Limited Edition",
        csv: include_str!("../data/headphones/Beyerdynamic DT770 Pro X Limited Edition.csv"),
    },
];

/// A built-in target curve (without bass shelf: that is the Bass control).
pub struct TargetDef {
    /// Display name.
    pub name: &'static str,
    /// Source and context of the curve.
    pub note: &'static str,
    csv: &'static str,
}

impl TargetDef {
    /// Parses the embedded curve.
    pub fn curve(&self) -> Curve {
        Curve::parse(self.csv).expect("embedded curve is valid")
    }
}

/// Built-in target curves.
pub static TARGETS: &[TargetDef] = &[
    TargetDef {
        name: "Harman OE 2018",
        note: "Harman over-ear 2018 (bass shelf is the Bass control)",
        csv: include_str!("../data/targets/Harman over-ear 2018 without bass.csv"),
    },
    TargetDef {
        name: "oratory1990 Optimum",
        note: "oratory1990 optimum hi-fi over-ear (bass shelf is the Bass control)",
        csv: include_str!("../data/targets/oratory1990 optimum hifi over-ear.csv"),
    },
];

/// Quick "sound" presets: (label, bass dB, tilt dB/oct).
pub static PRESETS: &[(&str, f64, f64)] =
    &[("Flat", 0.0, 0.0), ("Studio", 3.0, 0.0), ("Harman", 6.5, 0.0), ("Warm", 8.0, -0.3)];
