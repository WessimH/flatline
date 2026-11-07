//! Signal processing: everything here is pure Rust and platform independent.
//!
//! - [`biquad`]: RBJ cookbook filters (minimum phase, zero latency).
//! - [`curve`]: frequency-response curves on a log grid, parsing and smoothing.
//! - [`fit`]: builds the target and fits the correction EQ (Levenberg–Marquardt).
//! - [`processor`]: the allocation-free stereo processor run in the audio callback.

pub mod biquad;
pub mod curve;
pub mod fit;
pub mod processor;
