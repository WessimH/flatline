//! System-wide audio engine.
//!
//! Signal path (macOS 14.2+, no virtual driver needed):
//!
//! ```text
//!  all apps ──► CoreAudio process tap (muted while we read it)
//!                      │
//!            private aggregate device  ── one IO callback, one clock ──►  headphones
//!                      │                          ▲
//!                      └──► Processor (biquads) ──┘
//! ```

mod coreaudio;

pub use coreaudio::{Device, Engine, default_output_uid, list_output_devices};

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};

/// f32 stored in an `AtomicU32`.
#[derive(Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    /// Current value.
    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Relaxed))
    }
    /// Overwrites the value.
    pub fn store(&self, v: f32) {
        self.0.store(v.to_bits(), Relaxed)
    }
    /// Single-writer max (audio thread).
    pub fn store_max(&self, v: f32) {
        if v > self.load() {
            self.store(v)
        }
    }
    /// Read and reset (UI thread).
    pub fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0, Relaxed))
    }
}

/// Lock-free state shared between the UI and the audio callback.
#[derive(Default)]
pub struct Shared {
    /// Correction bypassed (written by the UI, read by the audio thread).
    pub bypass: AtomicBool,
    /// Input peak per channel since last read (linear).
    pub peak_in: [AtomicF32; 2],
    /// Output peak per channel since last read (linear).
    pub peak_out: [AtomicF32; 2],
    /// Number of output samples clamped to ±1.
    pub clips: AtomicU64,
    /// Max DSP load since last read (callback time / buffer duration).
    pub load: AtomicF32,
    /// Number of IO callbacks run (stalls mean the device stopped or macOS blocked it).
    pub callbacks: AtomicU64,
    /// Frames in the last IO callback.
    pub frames: AtomicU32,
    /// Output sample time − tap input sample time, in frames.
    pub io_offset: AtomicF32,
}
