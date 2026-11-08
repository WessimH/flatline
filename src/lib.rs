//! # Flatline
//!
//! System-wide headphone calibration for macOS, in the spirit of Sonarworks
//! SoundID Reference, with **zero added DSP latency**.
//!
//! Flatline captures what every app plays to your headphones, applies a
//! correction EQ computed from a measurement of your headphone model, and
//! sends the result back to the same device. The built-in profiles cover the
//! Beyerdynamic DT 770 PRO (oratory1990 measurements); any frequency-response
//! CSV can be dropped on the window.
//!
//! ## How the pieces fit
//!
//! ```text
//!  apps ──► CoreAudio process tap on the headphone output (muted while read)
//!                    │
//!        private aggregate device ── one IO callback, one clock ──► headphones
//!                    └──► dsp::processor::Processor (biquads) ──┘
//! ```
//!
//! | Module | Role |
//! |---|---|
//! | [`dsp`] | Biquad filters, frequency-response curves, the correction fitter and the real-time [`dsp::processor::Processor`]. Pure Rust, platform independent, unit tested. |
//! | [`engine`] | CoreAudio plumbing: process tap, private aggregate device, the real-time IO callback and the lock-free [`engine::Shared`] state. |
//! | [`analyzer`] | FFT real-time analyzer (1/6-octave RTA) fed from the audio thread. |
//! | [`profiles`] | Built-in headphone measurements, target curves and presets. |
//! | [`login`] | "Launch at login" through a per-user LaunchAgent. |
//! | [`log`] | Append-only diagnostic log in `~/Library/Logs/Flatline`. |
//! | [`ui`] | The egui interface, menu-bar icon and app icon. |
//!
//! ## Latency
//!
//! The correction is a cascade of minimum-phase IIR filters (RBJ biquads):
//! it adds **0 samples** of delay. The system path (tap → callback →
//! headphones) measures about 4.7 ms at 64 frames and 7.3 ms at 128 frames,
//! 48 kHz.
//!
//! ## Real-time safety
//!
//! The IO callback never allocates, locks or blocks. Settings reach it through
//! a wait-free SPSC ring buffer ([`rtrb`]); meters and status come back through
//! atomics. EQ changes are crossfaded over 1024 samples, bypass is a smoothed
//! wet/dry ramp, so nothing clicks.

#![warn(missing_docs)]

pub mod analyzer;
pub mod catalog;
pub mod dsp;
pub mod engine;
pub mod log;
pub mod login;
pub mod profiles;
pub mod ui;

/// Guide du code : quel fichier fait quoi, où modifier chaque écran,
/// comment tester (source : `docs/ARCHITECTURE.md`).
#[doc = include_str!("../docs/ARCHITECTURE.md")]
pub mod guide {}
