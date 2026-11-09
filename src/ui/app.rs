//! The main window: settings, engine lifecycle (start, restart on device or
//! sample-rate change, permission retries), menu-bar mode and every panel.

use super::graph::Layers;
use super::theme::install;
use super::tray::{self, Tray, TrayAction};
use crate::analyzer::Analyzer;
use crate::catalog::{Catalog, Entry, Form};
use crate::dsp::biquad::{Filter, cascade_response};
use crate::dsp::curve::{Curve, band_mean};
use crate::dsp::fit::{self, Design, DesignParams};
use crate::dsp::processor::EqSettings;
use crate::engine::{self, Device, Engine, Shared};
use crate::profiles;
use eframe::egui::{self, Frame, Key, Ui};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

mod onboarding;
mod views;

/// [`Settings::headphone_key`] of a dropped measurement.
const CUSTOM_KEY: &str = "custom";
/// Catalog key of the default headphones.
const DEFAULT_KEY: &str = "oratory1990/Beyerdynamic DT 770 Pro";
/// Keys of the four built-in measurements of earlier versions (v3 index).
const LEGACY_KEYS: [&str; 4] = [
    "oratory1990/Beyerdynamic DT 770 Pro",
    "oratory1990/Beyerdynamic DT 770 Pro (worn earpads)",
    "oratory1990/Beyerdynamic DT 770 Pro (Dekoni hybrid earpads)",
    "oratory1990/Beyerdynamic DT770 Pro X Limited Edition",
];
/// How long a freshly started engine may stay silent before it counts as
/// blocked. CoreAudio can take ~3 s to start IO on a tap aggregate (TCC check,
/// recording-indicator handshake): tearing it down earlier kills it just
/// before it starts.
const START_GRACE: Duration = Duration::from_secs(8);
/// Delay before retrying a blocked engine: 10 s, doubling up to 60 s.
fn retry_delay(retries: u32) -> Duration {
    Duration::from_secs((10u64 << retries.min(3)).min(60))
}
const SETTINGS_VERSION: u32 = 6;
const BUFFER_SIZES: [u32; 5] = [64, 128, 256, 512, 1024];
const SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";

// ----------------------------------------------------------- settings ----

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A measurement dropped on the window, kept with the settings.
pub struct CustomCurve {
    /// File name without extension.
    pub name: String,
    /// Raw CSV text.
    pub csv: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A profile: an output and the headphones used on it.
pub struct HeadphoneOutput {
    /// CoreAudio device UID.
    pub uid: String,
    /// Display name.
    pub name: String,
    /// The headphones on this output: a catalog key or `"custom"`. Empty in
    /// v5 files, filled by [`migrate`].
    #[serde(default)]
    pub headphones: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
/// Everything the user can change, persisted between launches.
pub struct Settings {
    /// Settings v3 and earlier: index into the old built-in list (or
    /// `usize::MAX` for a custom curve). Read once, migrated to
    /// [`Settings::headphone_key`], never written.
    #[serde(rename = "headphone", default, skip_serializing, deserialize_with = "legacy_index")]
    pub legacy_headphone: Option<usize>,
    /// The headphones: a catalog key ([`crate::catalog::Entry::key`]), or
    /// `"custom"` for a measurement dropped on the window.
    pub headphone_key: String,
    /// The user's headphones (catalog keys), listed in the switcher.
    pub my_headphones: Vec<String>,
    /// First-run setup done.
    pub onboarded: bool,
    /// 0 = the target of the measurement rig, 1 = oratory1990 Optimum
    /// (oratory1990 over-ear measurements and custom curves only).
    pub target: usize,
    /// Bass, tilt, boost limit and band count.
    pub params: DesignParams,
    /// Correction amount, 0 to 1 (scales every band's gain).
    pub amount: f64,
    /// Correction on (false = bypass).
    pub enabled: bool,
    /// Match the bypass loudness to the corrected signal for a fair A/B.
    pub level_match: bool,
    /// `None` = follow the system default output.
    pub output_uid: Option<String>,
    /// Requested IO buffer size, in frames.
    pub buffer_frames: u32,
    /// System-wide processing running.
    pub engine_on: bool,
    /// Measurement dropped on the window, if any.
    pub custom: Option<CustomCurve>,
    /// Visible layers of the frequency-response graph.
    pub layers: Layers,
    /// The headphones used on each output, learned when the user picks
    /// headphones while the system plays on that output. When the system
    /// switches to a listed output, its headphones are selected.
    pub headphone_outputs: Vec<HeadphoneOutput>,
    /// Settings v4: UID of the single headphone output. Read once, migrated
    /// to [`Settings::headphone_outputs`], never written.
    #[serde(rename = "headphone_output", skip_serializing)]
    pub legacy_headphone_output: Option<String>,
    /// Settings v4: display name of [`Settings::legacy_headphone_output`].
    #[serde(rename = "headphone_output_name", skip_serializing)]
    pub legacy_headphone_output_name: Option<String>,
    /// Field-level default (0) so files from older builds get migrated.
    #[serde(default)]
    pub version: u32,
}

/// Reads the bare integer older versions wrote (RON would expect `Some(n)`
/// for an `Option`, and a mismatch would discard every setting).
fn legacy_index<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<usize>, D::Error> {
    usize::deserialize(d).map(Some)
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            legacy_headphone: None,
            headphone_key: DEFAULT_KEY.into(),
            my_headphones: Vec::new(),
            onboarded: false,
            target: 0,
            // AutoEq's bass shelf for the Harman over-ear 2018 target.
            params: DesignParams { bass_db: 6.0, ..DesignParams::default() },
            amount: 1.0,
            enabled: true,
            level_match: true,
            output_uid: None,
            buffer_frames: 128,
            engine_on: true,
            custom: None,
            layers: Layers::default(),
            headphone_outputs: Vec::new(),
            legacy_headphone_output: None,
            legacy_headphone_output_name: None,
            version: SETTINGS_VERSION,
        }
    }
}

impl Settings {
    /// Remembers that `headphones` are used on this output (replaces the
    /// headphones it had).
    pub fn remember_output(&mut self, uid: &str, name: &str, headphones: &str) {
        match self.headphone_outputs.iter_mut().find(|o| o.uid == uid) {
            Some(o) => o.headphones = headphones.into(),
            None => self.headphone_outputs.push(HeadphoneOutput {
                uid: uid.into(),
                name: name.into(),
                headphones: headphones.into(),
            }),
        }
    }
}

/// Whether this output UID is the Mac's built-in speakers (never corrected).
fn is_speakers(uid: &str) -> bool {
    uid.contains("BuiltInSpeaker")
}

/// What the correction does when the system plays on `want`.
#[derive(Debug, PartialEq)]
enum Route {
    /// Keep correcting with the current headphones.
    Run,
    /// The built-in speakers: no correction.
    Pause,
    /// The output changed to one with other headphones: select them.
    Select(String),
    /// The output changed to one never seen: keep the headphones, correct,
    /// and ask whether other headphones are on it.
    NewOutput,
}

/// Decides the [`Route`] from the output audio goes to (`want`), whether it
/// changed since the last call, the remembered profiles and the selected
/// headphones (`key`).
fn route_decision(want: Option<&str>, changed: bool, outputs: &[HeadphoneOutput], key: &str) -> Route {
    match want {
        Some(w) if is_speakers(w) => Route::Pause,
        Some(w) if changed => match outputs.iter().find(|o| o.uid == w) {
            Some(o) if o.headphones != key => Route::Select(o.headphones.clone()),
            Some(_) => Route::Run,
            None => Route::NewOutput,
        },
        _ => Route::Run,
    }
}

/// Upgrades settings saved by an older version (no side effects).
fn migrate(s: &mut Settings) {
    if s.version < 2 {
        // v2: low-latency device tap → smaller default buffer.
        s.buffer_frames = 128;
    }
    if s.version < 4 {
        // v4: the AutoEq catalog replaces the four built-in measurements;
        // people upgrading have already set Flatline up.
        s.headphone_key = match s.legacy_headphone.take() {
            Some(usize::MAX) => CUSTOM_KEY.into(),
            Some(i) => LEGACY_KEYS[i.min(LEGACY_KEYS.len() - 1)].into(),
            None => DEFAULT_KEY.into(),
        };
        s.onboarded = true;
        // The old "Harman" preset (6.5 dB) becomes AutoEq's 6 dB.
        if (s.params.bass_db - 6.5).abs() < 1e-9 {
            s.params.bass_db = 6.0;
        }
    }
    if s.version < 5 {
        // v5: a list of headphone outputs replaces the single one.
        let name = s.legacy_headphone_output_name.take();
        if let Some(uid) = s.legacy_headphone_output.take() {
            s.remember_output(&uid, &name.unwrap_or_else(|| uid.clone()), "");
        }
    }
    if s.version < 6 {
        // v6: each output keeps its own headphones; the outputs learned so
        // far were all used with the current ones.
        let key = s.headphone_key.clone();
        for o in s.headphone_outputs.iter_mut().filter(|o| o.headphones.is_empty()) {
            o.headphones = key.clone();
        }
    }
    s.version = s.version.max(SETTINGS_VERSION);
}

// ----------------------------------------------------------- designer ----

#[derive(Clone, Debug, PartialEq)]
struct DesignKey {
    headphone: String,
    custom_len: usize,
    target: usize,
    params: DesignParams,
    fs: u32,
}

struct Job {
    key: DesignKey,
    hp: Curve,
    tg: Curve,
}

/// Fits run on a worker thread; only the latest request is computed.
struct Designer {
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<(DesignKey, Design)>,
}

impl Designer {
    fn new(ctx: egui::Context) -> Self {
        let (tx, job_rx) = mpsc::channel::<Job>();
        let (res_tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("designer".into())
            .spawn(move || {
                while let Ok(mut job) = job_rx.recv() {
                    while let Ok(newer) = job_rx.try_recv() {
                        job = newer;
                    }
                    let d = fit::design(&job.hp, &job.tg, &job.key.params, job.key.fs as f64);
                    if res_tx.send((job.key, d)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("spawn designer");
        Self { tx, rx }
    }
}

/// EQ for the audio thread: scaled bands, auto preamp, loudness-matched bypass gain.
fn build_eq(filters: &[Filter], amount: f64, level_match: bool, fs: f64) -> (EqSettings, f64) {
    let scaled: Vec<Filter> = filters.iter().map(|f| Filter { gain_db: f.gain_db * amount, ..*f }).collect();
    let preamp = -fit::max_gain_db(&scaled, fs).max(0.0) - if scaled.is_empty() { 0.0 } else { 0.3 };
    let dry = if level_match {
        let grid = crate::dsp::curve::log_grid();
        let resp = cascade_response(&scaled, &grid, fs);
        preamp + band_mean(&grid, &resp, 100.0, 8000.0)
    } else {
        0.0
    };
    (EqSettings::from_filters(&scaled, preamp, dry, fs), preamp)
}

// ------------------------------------------------------------ display ----

#[derive(Clone, Default)]
struct Disp {
    measured: Vec<f32>,
    target: Vec<f32>,
    eq: Vec<f32>,
    result: Vec<f32>,
}

impl Disp {
    /// Ease toward `goal`; returns true while still moving.
    fn approach(&mut self, goal: &Disp, k: f32) -> bool {
        let mut moving = false;
        for (a, b) in [
            (&mut self.measured, &goal.measured),
            (&mut self.target, &goal.target),
            (&mut self.eq, &goal.eq),
            (&mut self.result, &goal.result),
        ] {
            if a.len() != b.len() {
                *a = b.clone();
                continue;
            }
            for (x, y) in a.iter_mut().zip(b) {
                let d = y - *x;
                if d.abs() > 0.01 {
                    *x += d * k;
                    moving = true;
                } else {
                    *x = *y;
                }
            }
        }
        moving
    }
}

struct Meters {
    level: [f32; 4],
    hold: [f32; 4],
    hold_at: [Option<Instant>; 4],
}

impl Default for Meters {
    fn default() -> Self {
        Self { level: [-70.0; 4], hold: [-70.0; 4], hold_at: [None; 4] }
    }
}

impl Meters {
    fn update(&mut self, peaks: [f32; 4], dt: f32) {
        let now = Instant::now();
        for (i, peak) in peaks.into_iter().enumerate() {
            let db = 20.0 * peak.max(1e-6).log10();
            self.level[i] = if db > self.level[i] { db } else { (self.level[i] - 28.0 * dt).max(-70.0) };
            if db >= self.hold[i] {
                self.hold[i] = db;
                self.hold_at[i] = Some(now);
            } else if self.hold_at[i].is_none_or(|t| now - t > Duration::from_millis(1500)) {
                self.hold[i] = (self.hold[i] - 20.0 * dt).max(-70.0);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Status {
    Standby,
    Starting,
    Live,
    Blocked,
    Error,
    /// The Mac is asleep: CoreAudio pauses IO, nothing to retry until wake.
    Asleep,
    /// The system plays on the built-in speakers: no correction.
    Speakers,
}

// ---------------------------------------------------------------- app ----

/// The eframe application: UI, engine lifecycle and background design worker.
pub struct FlatlineApp {
    s: Settings,
    shared: Arc<Shared>,
    engine: Option<Engine>,
    engine_error: Option<String>,
    status: Status,
    last_restart: Instant,
    cb_seen: (u64, Instant),
    devices: Vec<Device>,
    default_uid: Option<String>,
    last_poll: Instant,
    last_device_scan: Instant,
    designer: Designer,
    requested: Option<DesignKey>,
    design: Option<Design>,
    design_gen: u64,
    eq_key: Option<(u64, u64, bool, u32)>,
    preamp_db: f64,
    goal: Disp,
    disp: Disp,
    analyzer: Analyzer,
    meters: Meters,
    load: f32,
    clips_seen: u64,
    clip_until: Option<Instant>,
    last_frame: Instant,
    custom_curve: Option<Curve>,
    /// Message, shown since, and whether it confirms a profile switch.
    toast: Option<(String, Instant, bool)>,
    /// Self-screenshot for testing: FL_SHOT=out.png (no OS permission needed).
    shot: Option<(std::path::PathBuf, u32, Arc<std::sync::atomic::AtomicBool>)>,
    /// Demo mode: factory settings, never persisted.
    demo: bool,
    /// Consecutive restarts that came up blocked (drives the tap fallback).
    blocked_retries: u32,
    /// Whether the current engine has delivered non-silent audio yet.
    signal_seen: bool,
    tray: Option<Tray>,
    /// System sleep / wake notifications.
    power: super::power::PowerWatch,
    started: Instant,
    /// Window hidden, running from the menu bar.
    hidden: bool,
    quitting: bool,
    login_enabled: bool,
    /// The living light behind the interface.
    aura: super::aura::Aura,
    /// Settings sheet open.
    settings_open: bool,
    /// Headphone switcher popover open.
    switcher_open: bool,
    /// The output [`FlatlineApp::route`] saw last.
    last_output: Option<String>,
    /// UID of a new output: the banner asks which headphones are on it.
    invite: Option<String>,
    /// Where the chart card was drawn last frame (toasts sit on it).
    chart_rect: egui::Rect,
    /// The switcher was drawn last frame (outside-click handling).
    switcher_shown: bool,
    /// Smoothed level bars for the Output card.
    bar_levels: Vec<f32>,
    /// Last clipping toast (rate limit).
    last_clip_toast: Option<Instant>,
    /// Demo only: FL_STATE=bypass|speakers|new-output|switched|blocked|error|asleep|
    /// switcher|settings|welcome|access|pick|sound forces a screen for
    /// screenshots.
    forced: Option<String>,
    /// First-run setup, or the headphone picker opened from the switcher.
    onb: Option<onboarding::Onboarding>,
}

impl FlatlineApp {
    /// Creates the app, restores the settings and starts the engine.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install(&cc.egui_ctx);
        // FL_DEMO=1: factory settings, nothing saved (screenshots, demos).
        let demo = std::env::var_os("FL_DEMO").is_some();
        crate::log::log(format!(
            "Flatline {} launched (background: {}, demo: {demo})",
            env!("CARGO_PKG_VERSION"),
            std::env::args().any(|a| a == "--background")
        ));
        let mut s: Settings = if demo {
            Settings::default()
        } else {
            cc.storage.and_then(|st| eframe::get_value(st, eframe::APP_KEY)).unwrap_or_default()
        };
        if s.version < 3 && crate::login::bundle_path().is_some() {
            // v3: menu-bar mode, launch at login on by default.
            let _ = crate::login::set_enabled(true);
        }
        migrate(&mut s);
        if crate::login::is_enabled() && crate::login::bundle_path().is_some() {
            let _ = crate::login::set_enabled(true); // refresh the path if the app moved
        }
        let background = std::env::args().any(|a| a == "--background");
        let tray = Tray::new(cc.egui_ctx.clone(), s.enabled);
        let power = super::power::PowerWatch::new(cc.egui_ctx.clone());
        if background && tray.is_some() {
            tray::set_dock_visible(false);
        }
        let custom_curve = s.custom.as_ref().and_then(|c| Curve::parse(&c.csv).ok());
        let shared = Arc::new(Shared::default());
        shared.bypass.store(!s.enabled, Relaxed);
        let now = Instant::now();
        let mut app = Self {
            shared,
            engine: None,
            engine_error: None,
            status: Status::Standby,
            last_restart: now - Duration::from_secs(10),
            cb_seen: (0, now),
            devices: engine::list_output_devices(),
            default_uid: engine::default_output_uid(),
            last_poll: now,
            last_device_scan: now,
            designer: Designer::new(cc.egui_ctx.clone()),
            requested: None,
            design: None,
            design_gen: 0,
            eq_key: None,
            preamp_db: 0.0,
            goal: Disp::default(),
            disp: Disp::default(),
            analyzer: Analyzer::new(48_000.0),
            meters: Meters::default(),
            load: 0.0,
            clips_seen: 0,
            clip_until: None,
            last_frame: now,
            custom_curve,
            toast: None,
            demo,
            blocked_retries: 0,
            signal_seen: false,
            shot: std::env::var_os("FL_SHOT").map(|p| (p.into(), 0, Default::default())),
            hidden: background && tray.is_some(),
            power,
            started: now,
            tray,
            quitting: false,
            login_enabled: crate::login::is_enabled(),
            aura: super::aura::Aura::default(),
            settings_open: false,
            switcher_open: false,
            last_output: None,
            invite: None,
            chart_rect: egui::Rect::NOTHING,
            switcher_shown: false,
            bar_levels: vec![0.0; 24],
            last_clip_toast: None,
            forced: if demo { std::env::var("FL_STATE").ok() } else { None },
            onb: None,
            s,
        };
        if (app.s.headphone_key == CUSTOM_KEY && app.custom_curve.is_none())
            || (app.s.headphone_key != CUSTOM_KEY && Catalog::get().find(&app.s.headphone_key).is_none())
        {
            app.s.headphone_key = DEFAULT_KEY.into();
        }
        let key = app.s.headphone_key.clone();
        app.remember_headphones(&key);
        app.s.target = app.s.target.min(1);
        if let Some(f) = app.forced.clone() {
            app.s.onboarded = !matches!(f.as_str(), "welcome" | "access" | "pick" | "sound");
        }
        if !app.s.onboarded {
            app.onb = Some(onboarding::Onboarding::new(&app.forced));
        }
        // First run: the engine (and the macOS permission prompt) waits for
        // the "Allow access" step.
        if app.s.engine_on && app.s.onboarded {
            app.start_engine();
        }
        app
    }

    fn fs(&self) -> f64 {
        self.engine.as_ref().map_or(48_000.0, |e| e.info.sample_rate)
    }

    /// The selected catalog entry (`None` for a custom measurement).
    fn entry(&self) -> Option<&'static Entry> {
        if self.s.headphone_key == CUSTOM_KEY {
            return None;
        }
        let cat = Catalog::get();
        cat.find(&self.s.headphone_key).or_else(|| cat.find(DEFAULT_KEY)).or_else(|| cat.entries().first())
    }

    /// Whether the oratory1990 Optimum target applies (GRAS over-ear data).
    fn optimum_allowed(&self) -> bool {
        self.entry().is_none_or(|e| e.source == "oratory1990" && e.form == Form::OverEar)
    }

    /// Measurement and target curves of the selected headphones.
    fn headphone_curves(&self) -> (Curve, Curve) {
        let optimum = self.s.target == 1 && self.optimum_allowed();
        match (self.entry(), &self.custom_curve) {
            (Some(e), _) => {
                let tg = if optimum { profiles::TARGETS[1].curve() } else { Catalog::get().target(e) };
                (Catalog::get().curve(e), tg)
            }
            (None, Some(c)) => (c.clone(), profiles::TARGETS[usize::from(optimum)].curve()),
            (None, None) => (profiles::HEADPHONES[0].curve(), profiles::TARGETS[0].curve()),
        }
    }

    /// UID of the output audio goes to: the chosen one if connected, else
    /// the system default.
    fn want_uid(&self) -> Option<String> {
        self.s.output_uid.clone().filter(|u| self.devices.iter().any(|d| &d.uid == u)).or(self.default_uid.clone())
    }

    /// Display name of headphones by key, e.g. `DT 770 Pro · Worn pads`.
    fn headphones_name(&self, key: &str) -> String {
        match (Catalog::get().find(key), &self.s.custom) {
            (Some(e), _) => e.variant.map_or_else(|| e.model.to_owned(), |v| format!("{} · {v}", e.model)),
            (None, Some(c)) if key == CUSTOM_KEY => c.name.clone(),
            _ => key.to_owned(),
        }
    }

    /// The Mac's name on its built-in speakers, e.g. `MacBook Pro`.
    fn speakers_name(&self) -> String {
        self.devices
            .iter()
            .find(|d| is_speakers(&d.uid))
            // "MacBook Pro Speakers", or "Haut-parleurs MacBook Pro" in French.
            .map_or("Mac".into(), |d| {
                d.name.trim_end_matches(" Speakers").trim_start_matches("Haut-parleurs ").to_owned()
            })
    }

    /// Forgets the headphones of an output. When it is the one playing, the
    /// correction goes on and the banner asks again.
    fn forget_output(&mut self, i: usize) {
        let o = self.s.headphone_outputs.remove(i);
        if self.want_uid().as_deref() == Some(o.uid.as_str()) {
            self.invite = Some(o.uid);
        }
    }

    /// Display name of an output ("" if not connected).
    fn device_name(&self, uid: &str) -> String {
        self.devices.iter().find(|d| d.uid == uid).map_or_else(String::new, |d| d.name.clone())
    }

    /// Display name of the selected headphones, e.g. `DT 770 Pro · Worn pads`.
    fn headphone_name(&self) -> String {
        let (model, variant) = self.headphone_label();
        match variant {
            Some(v) => format!("{model} · {v}"),
            None => model,
        }
    }

    /// Model and variant of the selected headphones.
    fn headphone_label(&self) -> (String, Option<String>) {
        match (self.entry(), &self.s.custom) {
            (Some(e), _) => (e.model.to_owned(), e.variant.map(str::to_owned)),
            (None, Some(c)) => (c.name.clone(), Some("Your measurement".into())),
            (None, None) => ("Headphones".into(), None),
        }
    }

    /// Name of the target in use.
    fn target_name(&self) -> String {
        if self.s.target == 1 && self.optimum_allowed() {
            return profiles::TARGETS[1].name.into();
        }
        self.entry().map_or(profiles::TARGETS[0].name, |e| Catalog::get().target_name(e)).to_owned()
    }

    /// Selects headphones by catalog key (or [`CUSTOM_KEY`]) and remembers
    /// them for the output audio goes to (never the built-in speakers).
    fn select_headphones(&mut self, key: &str) {
        if let Some(uid) = self.want_uid().filter(|u| !is_speakers(u)) {
            let name = self.device_name(&uid);
            self.s.remember_output(&uid, if name.is_empty() { &uid } else { &name }, key);
        }
        self.invite = None;
        if key != self.s.headphone_key {
            self.s.headphone_key = key.to_owned();
            if !self.optimum_allowed() {
                self.s.target = 0;
            }
        }
        self.remember_headphones(key);
    }

    /// Puts `key` at the top of the switcher list (catalog keys only).
    fn remember_headphones(&mut self, key: &str) {
        if key == CUSTOM_KEY {
            return;
        }
        self.s.my_headphones.retain(|k| k != key);
        self.s.my_headphones.insert(0, key.to_owned());
        self.s.my_headphones.truncate(8);
    }

    fn design_key(&self) -> DesignKey {
        DesignKey {
            headphone: self.s.headphone_key.clone(),
            custom_len: self.s.custom.as_ref().map_or(0, |c| c.csv.len() ^ c.name.len() << 20),
            target: self.s.target,
            params: self.s.params,
            fs: self.fs() as u32,
        }
    }

    fn start_engine(&mut self) {
        self.engine = None; // tear down the previous tap/aggregate first
        self.last_restart = Instant::now();
        // Device tap first; if it keeps coming up blocked, alternate with the
        // global-mix tap (some device states refuse the device tap).
        let prefer_device_tap = self.blocked_retries < 2 || self.blocked_retries % 2 == 1;
        let filters = self.design.as_ref().map(|d| d.filters.clone()).unwrap_or_default();
        let (amount, lm) = (self.s.amount, self.s.level_match);
        let uid = self.s.output_uid.clone().filter(|u| self.devices.iter().any(|d| &d.uid == u));
        let frames = std::env::var("FL_BUF").ok().and_then(|v| v.parse().ok()).unwrap_or(self.s.buffer_frames);
        match Engine::start(uid.as_deref(), frames, prefer_device_tap, self.shared.clone(), move |fs| {
            build_eq(&filters, amount, lm, fs).0
        }) {
            Ok(e) => {
                crate::log::log(format!(
                    "engine started: {} ({}) · {:.0} Hz · {} frames · {} · in {:?} out {:?}",
                    e.info.device.name,
                    e.info.device.uid,
                    e.info.sample_rate,
                    e.info.buffer_frames,
                    e.info.tap_mode,
                    e.info.in_layout,
                    e.info.out_layout
                ));
                self.analyzer.set_rate(e.info.sample_rate);
                self.engine = Some(e);
                self.engine_error = None;
                self.status = Status::Starting;
                self.signal_seen = false;
                self.cb_seen = (self.shared.callbacks.load(Relaxed), Instant::now());
                self.eq_key = None;
            }
            Err(err) => {
                crate::log::log(format!("engine failed to start: {err:#}"));
                self.engine_error = Some(format!("{err:#}"));
                self.status = Status::Error;
            }
        }
    }

    /// Correction follows the output: none on the built-in speakers; on an
    /// output with remembered headphones, select them; on a new output, keep
    /// the headphones and invite the user to change them. Returns true while
    /// paused.
    fn route(&mut self) -> bool {
        if !self.s.engine_on || self.power.is_sleeping() {
            return false;
        }
        let want = self.want_uid();
        let changed = want != self.last_output;
        self.last_output = want.clone();
        if changed {
            self.invite = None;
        }
        let paused = self.status == Status::Speakers;
        match route_decision(want.as_deref(), changed, &self.s.headphone_outputs, &self.s.headphone_key) {
            Route::Pause => {
                if !paused || self.engine.is_some() {
                    crate::log::log("built-in speakers: no correction");
                    self.engine = None;
                    self.status = Status::Speakers;
                }
                return true;
            }
            Route::Select(key) => {
                let known =
                    if key == CUSTOM_KEY { self.custom_curve.is_some() } else { Catalog::get().find(&key).is_some() };
                if known {
                    crate::log::log(format!("output {}: headphones {key}", want.as_deref().unwrap_or("?")));
                    self.select_headphones(&key);
                    self.notify_switched();
                }
            }
            // During the first-run setup, picking headphones remembers them.
            Route::NewOutput if self.onb.is_none() => self.invite = want.clone(),
            Route::NewOutput | Route::Run => {}
        }
        if paused {
            crate::log::log("left the built-in speakers: resuming");
            self.start_engine();
        }
        false
    }

    fn stop_engine(&mut self) {
        self.engine = None;
        self.status = Status::Standby;
    }

    fn update_goal(&mut self) {
        let Some(d) = &self.design else { return };
        let scaled = d.scaled_filters(self.s.amount);
        let eq = cascade_response(&scaled, &d.grid, d.fs);
        self.goal = Disp {
            measured: d.measured.iter().map(|&v| v as f32).collect(),
            target: d.target.iter().map(|&v| v as f32).collect(),
            result: d.measured.iter().zip(&eq).map(|(m, e)| (m + e) as f32).collect(),
            eq: eq.into_iter().map(|v| v as f32).collect(),
        };
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        tray::set_dock_visible(true);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.hidden = false;
    }

    fn hide_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        tray::set_dock_visible(false);
        self.hidden = true;
    }

    fn tray_status(&self) -> String {
        let device = self.engine.as_ref().map_or(String::new(), |e| e.info.device.name.clone());
        match self.status {
            Status::Live => format!("● Live · {device}"),
            Status::Starting => "Starting…".into(),
            Status::Blocked => "Blocked by macOS · open Flatline".into(),
            Status::Error => "Engine error · open Flatline".into(),
            Status::Standby => "Standby (system audio untouched)".into(),
            Status::Asleep => "Paused · Mac asleep".into(),
            Status::Speakers => "Paused · speakers aren't corrected".into(),
        }
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now(), false));
    }

    /// Toast with a check mark: the profile of the new output is loaded.
    fn notify_switched(&mut self) {
        let msg = format!("Switched to {} profile", self.headphone_label().0);
        self.toast = Some((msg, Instant::now(), true));
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            let name =
                f.path().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Custom".into());
            let text = f.bytes().map_err(anyhow::Error::msg).map(|b| String::from_utf8_lossy(&b).into_owned());
            self.load_measurement(name, text);
        }
    }

    /// Uses a frequency-response CSV (dropped or browsed) as the headphones.
    fn load_measurement(&mut self, name: String, text: anyhow::Result<String>) {
        match text.and_then(|t| Curve::parse(&t).map(|c| (c, t))) {
            Ok((curve, csv)) => {
                self.custom_curve = Some(curve);
                self.s.custom = Some(CustomCurve { name: name.clone(), csv });
                self.select_headphones(CUSTOM_KEY);
                if let Some(o) = &mut self.onb {
                    o.custom_loaded();
                }
                self.notify(format!("Loaded measurement “{name}”"));
            }
            Err(e) => self.notify(format!("Could not read {name}: {e}")),
        }
    }

    /// Demo screenshots of every state (see [`FlatlineApp::forced`]).
    fn apply_forced(&mut self) {
        let Some(f) = self.forced.clone() else { return };
        let paused = |app: &mut Self, st: Status| {
            app.engine = None;
            app.status = st;
        };
        // Two profiles: the output playing now, and headphones not connected.
        if f != "new-output" && self.s.headphone_outputs.is_empty() {
            if let Some(uid) = self.default_uid.clone().filter(|u| !is_speakers(u)) {
                let name = self.device_name(&uid);
                self.s.remember_output(&uid, &name, DEFAULT_KEY);
            }
            if let Some(e) = Catalog::get().entries().iter().find(|e| e.model == "WH-1000XM5" && e.variant.is_none()) {
                self.s.remember_output("demo-sony", "WH-1000XM5", &e.key());
                self.s.my_headphones.push(e.key());
            }
        }
        match f.as_str() {
            "bypass" => self.s.enabled = false,
            "new-output" => self.invite = self.default_uid.clone(),
            "switched" if self.toast.is_none() => self.notify_switched(),
            "speakers" => paused(self, Status::Speakers),
            "blocked" => paused(self, Status::Blocked),
            "error" => {
                self.engine_error = Some("AudioDeviceStart: 'stop' (device is hogged by another process)".into());
                paused(self, Status::Error);
            }
            "asleep" => paused(self, Status::Asleep),
            "switcher" => self.switcher_open = true,
            "settings" => self.settings_open = true,
            _ => {}
        }
    }

    /// Output card bars: the post-EQ spectrum in 24 log bands, smoothed.
    fn update_bars(&mut self, dt: f32) {
        let n = self.bar_levels.len();
        let post = &self.analyzer.db[1];
        let live = self.status == Status::Live && !post.is_empty();
        for (i, bar) in self.bar_levels.iter_mut().enumerate() {
            let goal = if live {
                let (a, b) = (i * post.len() / n, ((i + 1) * post.len() / n).max(i * post.len() / n + 1));
                let db = post[a..b.min(post.len())].iter().copied().fold(f32::MIN, f32::max);
                ((db + 84.0) / 64.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let k = if goal > *bar { 1.0 - (-dt / 0.03).exp() } else { 1.0 - (-dt / 0.25).exp() };
            *bar += (goal - *bar) * k;
        }
    }

    fn eq_text(&self) -> String {
        let Some(d) = &self.design else { return String::new() };
        let mut s = format!("Preamp: {:.1} dB\n", self.preamp_db);
        for (i, f) in d.scaled_filters(self.s.amount).iter().enumerate() {
            s += &format!(
                "Filter {}: ON {} Fc {:.0} Hz Gain {:.1} dB Q {:.2}\n",
                i + 1,
                f.kind.short(),
                f.freq,
                f.gain_db,
                f.q
            );
        }
        s
    }
}

impl eframe::App for FlatlineApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;

        // Correction design (worker thread).
        let key = self.design_key();
        if self.requested.as_ref() != Some(&key) {
            let (hp, tg) = self.headphone_curves();
            let _ = self.designer.tx.send(Job { key: key.clone(), hp, tg });
            self.requested = Some(key);
        }
        while let Ok((k, d)) = self.designer.rx.try_recv() {
            if self.requested.as_ref() == Some(&k) {
                self.design = Some(d);
                self.design_gen += 1;
            }
        }

        // Push EQ to the audio thread when anything relevant changed.
        let eq_key = (self.design_gen, self.s.amount.to_bits(), self.s.level_match, self.fs() as u32);
        if self.eq_key != Some(eq_key) && self.design.is_some() {
            let fs = self.fs();
            let filters = self.design.as_ref().map(|d| d.filters.clone()).unwrap_or_default();
            let (eq, pre) = build_eq(&filters, self.s.amount, self.s.level_match, fs);
            self.preamp_db = pre;
            if let Some(e) = &mut self.engine {
                e.send_eq(eq);
            }
            self.update_goal();
            self.eq_key = Some(eq_key);
        }
        if self.disp.approach(&self.goal, 0.22) {
            ctx.request_repaint();
        }

        self.shared.bypass.store(!self.s.enabled, Relaxed);

        // Engine health, device changes.
        if now - self.last_poll > Duration::from_millis(250) {
            self.last_poll = now;
            if now - self.last_device_scan > Duration::from_secs(2) {
                self.last_device_scan = now;
                self.devices = engine::list_output_devices();
            }
            self.default_uid = engine::default_output_uid();
            let routed_away = self.forced.is_none() && self.route();
            if self.power.take_woke() {
                crate::log::log("system woke up: restarting the engine");
                self.blocked_retries = 0;
                if self.s.engine_on && !routed_away {
                    self.start_engine();
                }
            }
            if self.power.is_sleeping() {
                if self.engine.is_some() && self.status != Status::Asleep {
                    crate::log::log("system going to sleep: engine paused");
                    self.status = Status::Asleep;
                }
            } else if let Some(e) = &self.engine {
                let cb = self.shared.callbacks.load(Relaxed);
                let before = self.status;
                if cb != self.cb_seen.0 {
                    self.cb_seen = (cb, now);
                    self.status = Status::Live;
                    self.blocked_retries = 0;
                } else if self.status == Status::Live && now - self.cb_seen.1 > Duration::from_millis(1500) {
                    // Was running, then stopped (device gone, coreaudiod restart…).
                    self.status = Status::Blocked;
                } else if self.status != Status::Live && now - self.last_restart > START_GRACE {
                    self.status = Status::Blocked;
                }
                if self.status != before {
                    let since_start = (now - self.last_restart).as_millis();
                    crate::log::log(format!(
                        "status {before:?} → {:?} ({}, {since_start} ms after start)",
                        self.status, e.info.tap_mode
                    ));
                }
                let retry_blocked = self.status == Status::Blocked
                    && now - self.last_restart > START_GRACE + retry_delay(self.blocked_retries);
                let want = self.want_uid();
                let moved = want.as_deref().is_some_and(|w| w != e.info.device.uid);
                let rate_changed = e.current_sample_rate().is_some_and(|r| (r - e.info.sample_rate).abs() > 1.0);
                let alive = e.device_alive();
                if retry_blocked
                    || ((moved || rate_changed || !alive) && now - self.last_restart > Duration::from_millis(1500))
                {
                    let why = if retry_blocked {
                        self.blocked_retries += 1;
                        format!("still blocked (retry {})", self.blocked_retries)
                    } else if moved {
                        format!("output moved to {}", want.as_deref().unwrap_or("?"))
                    } else if rate_changed {
                        "sample rate changed".to_owned()
                    } else {
                        "device disconnected".to_owned()
                    };
                    crate::log::log(format!("restarting engine: {why}"));
                    self.start_engine();
                }
            }
        }

        // Menu bar: red close button hides to the menu bar instead of quitting.
        if self.tray.is_some() && !self.quitting && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_window(ctx);
        }
        for action in self.tray.as_ref().map(|t| t.actions()).unwrap_or_default() {
            match action {
                TrayAction::Show => self.show_window(ctx),
                TrayAction::ToggleCorrection => self.s.enabled = !self.s.enabled,
                TrayAction::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        let status_line = self.tray_status();
        let enabled = self.s.enabled;
        if let Some(t) = &mut self.tray {
            t.sync(enabled, &status_line);
        }

        // Analyzer + meters (skipped while hidden).
        let mut got = 0;
        if let Some(e) = &mut self.engine {
            while let Ok(s) = e.scope.pop() {
                if !self.hidden {
                    self.analyzer.push(s);
                }
                got += 1;
            }
        }
        if self.hidden {
        } else if got > 0 {
            self.analyzer.compute()
        } else {
            self.analyzer.decay()
        }
        let sh = &self.shared;
        let peaks = [sh.peak_in[0].take(), sh.peak_in[1].take(), sh.peak_out[0].take(), sh.peak_out[1].take()];
        if !self.signal_seen && self.engine.is_some() && peaks[0].max(peaks[1]) > 1e-4 {
            self.signal_seen = true;
            crate::log::log(format!(
                "first audio from the tap {} ms after start",
                (now - self.last_restart).as_millis()
            ));
        }
        self.meters.update(peaks, dt);
        let out_peak = peaks[2].max(peaks[3]);
        let load = sh.load.take();
        self.load = if load > self.load { load } else { self.load + (load - self.load) * 0.05 };
        let clips = sh.clips.load(Relaxed);
        if clips != self.clips_seen {
            self.clips_seen = clips;
            self.clip_until = Some(now + Duration::from_millis(1500));
            if self.last_clip_toast.is_none_or(|t| now - t > Duration::from_secs(30)) {
                self.last_clip_toast = Some(now);
                self.notify("Output is clipping · lower Bass or Max boost");
            }
        }

        let mood = if self.in_setup() { super::aura::Mood::Live } else { self.mood() };
        self.aura.update(mood, out_peak, dt);
        if self.aura.settling(mood) {
            ctx.request_repaint();
        }
        self.update_bars(dt);

        if ctx.input(|i| i.key_pressed(Key::Space)) && !ctx.egui_wants_keyboard_input() {
            self.s.enabled = !self.s.enabled;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.switcher_open = false;
            self.settings_open = false;
        }
        self.handle_drops(ctx);

        self.apply_forced();

        let report = format!(
            "status={:?} hidden={} tray={} login={} io_offset={} fs={} buffer={} load={:.3}\n",
            self.status,
            self.hidden,
            self.tray.is_some(),
            self.login_enabled,
            self.shared.io_offset.load(),
            self.fs(),
            self.engine.as_ref().map_or(0, |e| e.info.buffer_frames),
            self.load
        );
        let since_start = self.started.elapsed().as_secs_f64();
        if let Some((path, fired, done)) = &mut self.shot {
            let wait: f64 = std::env::var("FL_SHOT_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(3.0);
            if *fired == 0 && since_start >= wait {
                *fired = 1;
                let _ = std::fs::write(path.with_extension("txt"), report);
                let (path, done) = (path.clone(), done.clone());
                ctx.request_screenshot(move |img| {
                    let [w, h] = img.size;
                    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
                    let _ = super::icon::write_png(&path, w as u32, h as u32, &rgba);
                    done.store(true, Relaxed);
                });
            }
            if done.load(Relaxed) {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }

        ctx.request_repaint_after(if self.hidden {
            Duration::from_millis(250)
        } else if self.status == Status::Live {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(33) // the light keeps drifting
        });
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().frame(Frame::NONE).show(ui, |ui| {
            if self.onb.is_some() {
                self.onboarding_view(ui);
            } else {
                self.view(ui);
            }
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if !self.demo {
            eframe::set_value(storage, eframe::APP_KEY, &self.s);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        crate::log::log("quit");
        self.engine = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_settings_move_to_the_catalog() {
        for (legacy, key) in
            [(Some(0), LEGACY_KEYS[0]), (Some(1), LEGACY_KEYS[1]), (Some(3), LEGACY_KEYS[3]), (None, DEFAULT_KEY)]
        {
            let mut s = Settings {
                legacy_headphone: legacy,
                version: 3,
                params: DesignParams { bass_db: 6.5, ..DesignParams::default() },
                ..Settings::default()
            };
            migrate(&mut s);
            assert_eq!(s.headphone_key, key);
            assert!(Catalog::get().find(&s.headphone_key).is_some(), "{key} is in the catalog");
            assert!(s.onboarded, "upgrades skip the first-run setup");
            assert_eq!(s.params.bass_db, 6.0);
            assert_eq!(s.version, SETTINGS_VERSION);
        }
        let mut custom = Settings { legacy_headphone: Some(usize::MAX), version: 3, ..Settings::default() };
        migrate(&mut custom);
        assert_eq!(custom.headphone_key, CUSTOM_KEY);
    }

    fn profile(uid: &str, headphones: &str) -> HeadphoneOutput {
        HeadphoneOutput { uid: uid.into(), name: uid.into(), headphones: headphones.into() }
    }

    #[test]
    fn correction_follows_the_output() {
        let (usb, jack, spk, air) = ("Scarlett", "BuiltInHeadphoneOutputDevice", "BuiltInSpeakerDevice", "AirPods");
        let (dt770, apple) = (DEFAULT_KEY, "Apple AirPods Pro");
        let profiles = [profile(usb, dt770), profile(jack, dt770), profile(air, apple)];
        // The built-in speakers are never corrected, known or not.
        assert_eq!(route_decision(Some(spk), true, &[], dt770), Route::Pause);
        assert_eq!(route_decision(Some(spk), false, &profiles, dt770), Route::Pause);
        // Same headphones on the Scarlett and on the jack: nothing to change.
        assert_eq!(route_decision(Some(usb), true, &profiles, dt770), Route::Run);
        assert_eq!(route_decision(Some(jack), true, &profiles, dt770), Route::Run);
        // Switch to the AirPods: their profile is selected, and back.
        assert_eq!(route_decision(Some(air), true, &profiles, dt770), Route::Select(apple.into()));
        assert_eq!(route_decision(Some(usb), true, &profiles, apple), Route::Select(dt770.into()));
        // Bluetooth or wired, a new output keeps the headphones and is corrected.
        assert_eq!(route_decision(Some("Sony WH-1000XM5"), true, &profiles, dt770), Route::NewOutput);
        assert_eq!(route_decision(Some(usb), true, &[], dt770), Route::NewOutput);
        // Only a change of output selects or invites: the user's choice stays.
        assert_eq!(route_decision(Some(air), false, &profiles, dt770), Route::Run);
        assert_eq!(route_decision(Some("Sony WH-1000XM5"), false, &profiles, dt770), Route::Run);
        assert_eq!(route_decision(None, true, &profiles, dt770), Route::Run);
    }

    #[test]
    fn one_profile_per_output() {
        let mut s = Settings::default();
        s.remember_output("Scarlett", "Scarlett Solo USB", DEFAULT_KEY);
        s.remember_output("AirPods", "AirPods Pro", "Apple AirPods Pro");
        s.remember_output("Scarlett", "Scarlett Solo USB", LEGACY_KEYS[1]);
        assert_eq!(
            s.headphone_outputs,
            [
                HeadphoneOutput {
                    uid: "Scarlett".into(),
                    name: "Scarlett Solo USB".into(),
                    headphones: LEGACY_KEYS[1].into()
                },
                HeadphoneOutput {
                    uid: "AirPods".into(),
                    name: "AirPods Pro".into(),
                    headphones: "Apple AirPods Pro".into()
                },
            ]
        );
    }

    /// A real settings file written by v5: every output gets the headphones
    /// selected then.
    #[test]
    fn real_v5_file_gives_each_output_the_headphones() {
        let v5 = "(headphone_key:\"oratory1990/Beyerdynamic DT 770 Pro (worn earpads)\",\
                  my_headphones:[\"oratory1990/Beyerdynamic DT 770 Pro (worn earpads)\"],onboarded:true,target:0,\
                  params:(bass_db:6.0,tilt_db_oct:0.0,max_boost_db:8.0,n_filters:10),amount:0.8,enabled:true,\
                  level_match:true,output_uid:Some(\"BuiltInHeadphoneOutputDevice\"),buffer_frames:128,\
                  engine_on:true,custom:None,layers:(measured:true,target:true,eq:true,result:false,rta:false),\
                  headphone_outputs:[(uid:\"AppleUSBAudioEngine:Focusrite:Scarlett Solo USB:1\",\
                  name:\"Scarlett Solo USB\"),(uid:\"BuiltInHeadphoneOutputDevice\",name:\"MacBook Pro Headphones\")],\
                  version:5)";
        let mut s: Settings = ron::from_str(v5).expect("v5 settings parse");
        migrate(&mut s);
        let worn = LEGACY_KEYS[1];
        assert_eq!(
            s.headphone_outputs,
            [
                HeadphoneOutput {
                    uid: "AppleUSBAudioEngine:Focusrite:Scarlett Solo USB:1".into(),
                    name: "Scarlett Solo USB".into(),
                    headphones: worn.into()
                },
                HeadphoneOutput {
                    uid: "BuiltInHeadphoneOutputDevice".into(),
                    name: "MacBook Pro Headphones".into(),
                    headphones: worn.into()
                },
            ]
        );
        assert_eq!(s.output_uid.as_deref(), Some("BuiltInHeadphoneOutputDevice"));
        assert_eq!((s.headphone_key.as_str(), s.amount, s.version), (worn, 0.8, SETTINGS_VERSION));
        let back: Settings = ron::from_str(&ron::to_string(&s).expect("serialize")).expect("v6 settings parse");
        assert_eq!(back, s);
    }

    /// A real settings file written by v4, with and without a headphone output.
    #[test]
    fn real_v4_file_moves_the_output_to_the_list() {
        let v4 = |out: &str| {
            format!(
                "(headphone_key:\"oratory1990/Beyerdynamic DT 770 Pro\",my_headphones:[],onboarded:true,target:0,\
                 params:(bass_db:6.0,tilt_db_oct:0.0,max_boost_db:8.0,n_filters:10),amount:0.8,enabled:true,\
                 level_match:true,output_uid:None,buffer_frames:128,engine_on:true,custom:None,\
                 layers:(measured:true,target:true,eq:true,result:false,rta:false),{out}version:4)"
            )
        };
        let with = v4("headphone_output:Some(\"AppleUSBAudioEngine:Focusrite:Scarlett Solo USB:1\"),\
                       headphone_output_name:Some(\"Scarlett Solo USB\"),");
        let mut s: Settings = ron::from_str(&with).expect("v4 settings parse");
        migrate(&mut s);
        assert_eq!(
            s.headphone_outputs,
            [HeadphoneOutput {
                uid: "AppleUSBAudioEngine:Focusrite:Scarlett Solo USB:1".into(),
                name: "Scarlett Solo USB".into(),
                headphones: DEFAULT_KEY.into()
            }]
        );
        assert_eq!((s.amount, s.version), (0.8, SETTINGS_VERSION));
        // Round trip: the legacy fields are never written back.
        let saved = ron::to_string(&s).expect("serialize");
        assert!(!saved.contains("headphone_output:"), "{saved}");
        let back: Settings = ron::from_str(&saved).expect("v5 settings parse");
        assert_eq!(back, s);

        let without = v4("headphone_output:None,headphone_output_name:None,");
        let mut s: Settings = ron::from_str(&without).expect("v4 settings parse");
        migrate(&mut s);
        assert!(s.headphone_outputs.is_empty());
        assert_eq!(s.amount, 0.8);
    }

    /// A real settings file written by v3 (the bug: it failed to load).
    #[test]
    fn real_v3_file_loads_and_keeps_its_values() {
        let v3 = "(headphone:2,target:0,params:(bass_db:0.0,tilt_db_oct:0.0,max_boost_db:8.0,n_filters:10),\
                  amount:1.0,enabled:false,level_match:true,output_uid:None,buffer_frames:256,engine_on:true,\
                  custom:None,layers:(measured:true,target:true,eq:true,result:false,rta:false),version:3)";
        let mut s: Settings = ron::from_str(v3).expect("v3 settings parse");
        migrate(&mut s);
        assert_eq!(s.headphone_key, LEGACY_KEYS[2]);
        assert!(s.onboarded);
        assert_eq!((s.params.bass_db, s.enabled, s.buffer_frames), (0.0, false, 256));
        // Round trip: the legacy field is never written back.
        let saved = ron::to_string(&s).expect("serialize");
        assert!(!saved.contains("headphone:"), "{saved}");
        let back: Settings = ron::from_str(&saved).expect("v4 settings parse");
        assert_eq!(back, s);
    }

    #[test]
    fn fresh_settings_need_setup() {
        let mut s = Settings::default();
        migrate(&mut s);
        assert!(!s.onboarded);
        assert_eq!(s.headphone_key, DEFAULT_KEY);
    }

    #[test]
    fn presets_follow_the_target_bass() {
        let over = onboarding::presets(6.0);
        assert_eq!([over[0].bass, over[1].bass, over[2].bass, over[3].bass], [0.0, 3.0, 6.0, 7.5]);
        assert_eq!(onboarding::presets(8.0)[2].bass, 8.0);
    }
}
