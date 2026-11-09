//! System sleep / wake tracking.
//!
//! While the Mac sleeps (lid closed, idle sleep, Power Nap "dark wakes"),
//! CoreAudio stops or silences IO: an engine that gets no callbacks then is
//! not blocked, just paused. The app uses this to show "Paused · Mac asleep",
//! stop retrying, and restart the engine once on wake.

use block2::RcBlock;
use eframe::egui;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification};
use objc2_foundation::{NSNotification, NSObjectProtocol};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

/// Observes `NSWorkspace` sleep and wake notifications. The default value
/// observes nothing: the Mac never sleeps (simulated devices, tests).
#[derive(Default)]
pub struct PowerWatch {
    sleeping: Arc<AtomicBool>,
    woke: Arc<AtomicBool>,
    _tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl PowerWatch {
    /// Starts observing; `ctx` is woken so the app reacts even when hidden.
    pub fn new(ctx: egui::Context) -> Self {
        let sleeping = Arc::new(AtomicBool::new(false));
        let woke = Arc::new(AtomicBool::new(false));
        let center = NSWorkspace::sharedWorkspace().notificationCenter();

        let (s, c) = (sleeping.clone(), ctx.clone());
        let on_sleep = RcBlock::new(move |_: NonNull<NSNotification>| {
            s.store(true, Relaxed);
            c.request_repaint();
        });
        let (s, w) = (sleeping.clone(), woke.clone());
        let on_wake = RcBlock::new(move |_: NonNull<NSNotification>| {
            s.store(false, Relaxed);
            w.store(true, Relaxed);
            ctx.request_repaint();
        });
        // SAFETY: the notification names are AppKit statics, the blocks are
        // 'static and only touch atomics and a thread-safe egui context.
        let tokens = unsafe {
            vec![
                center.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceWillSleepNotification),
                    None,
                    None,
                    &on_sleep,
                ),
                center.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceDidWakeNotification),
                    None,
                    None,
                    &on_wake,
                ),
            ]
        };
        Self { sleeping, woke, _tokens: tokens }
    }

    /// The system announced sleep and has not woken since.
    pub fn is_sleeping(&self) -> bool {
        self.sleeping.load(Relaxed)
    }

    /// True once after each wake.
    pub fn take_woke(&self) -> bool {
        self.woke.swap(false, Relaxed)
    }
}
