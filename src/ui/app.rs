//! The main window: settings, engine lifecycle (start, restart on device or
//! sample-rate change, permission retries), menu-bar mode and every panel.

use super::graph::{self, Layers};
use super::theme::*;
use super::tray::{self, Tray, TrayAction};
use super::widgets;
use crate::analyzer::Analyzer;
use crate::dsp::biquad::{Filter, FilterKind, cascade_response};
use crate::dsp::curve::{Curve, band_mean};
use crate::dsp::fit::{self, Design, DesignParams};
use crate::dsp::processor::EqSettings;
use crate::engine::{self, Device, Engine, Shared};
use crate::profiles;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Frame, Key, Layout, Margin, Pos2, Rect, RichText, ScrollArea, Sense,
    Stroke, Ui, Vec2,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const CUSTOM: usize = usize::MAX;
const SETTINGS_VERSION: u32 = 3;
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
#[serde(default)]
/// Everything the user can change, persisted between launches.
pub struct Settings {
    /// Index into [`crate::profiles::HEADPHONES`], or a custom curve.
    pub headphone: usize,
    /// Index into [`crate::profiles::TARGETS`].
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
    /// Field-level default (0) so files from older builds get migrated.
    #[serde(default)]
    pub version: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            headphone: 0,
            target: 0,
            params: DesignParams::default(),
            amount: 1.0,
            enabled: true,
            level_match: true,
            output_uid: None,
            buffer_frames: 128,
            engine_on: true,
            custom: None,
            layers: Layers::default(),
            version: SETTINGS_VERSION,
        }
    }
}

// ----------------------------------------------------------- designer ----

#[derive(Clone, Debug, PartialEq)]
struct DesignKey {
    headphone: usize,
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
    toast: Option<(String, Instant)>,
    /// Self-screenshot for testing: FL_SHOT=out.png (no OS permission needed).
    shot: Option<(std::path::PathBuf, u32, Arc<std::sync::atomic::AtomicBool>)>,
    /// Demo mode: factory settings, never persisted.
    demo: bool,
    tray: Option<Tray>,
    started: Instant,
    /// Window hidden, running from the menu bar.
    hidden: bool,
    quitting: bool,
    login_enabled: bool,
}

impl FlatlineApp {
    /// Creates the app, restores the settings and starts the engine.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install(&cc.egui_ctx);
        // FL_DEMO=1: factory settings, nothing saved (screenshots, demos).
        let demo = std::env::var_os("FL_DEMO").is_some();
        let mut s: Settings = if demo {
            Settings::default()
        } else {
            cc.storage.and_then(|st| eframe::get_value(st, eframe::APP_KEY)).unwrap_or_default()
        };
        if s.version < 2 {
            // v2: low-latency device tap → smaller default buffer.
            s.buffer_frames = 128;
            s.version = 2;
        }
        if s.version < 3 && crate::login::bundle_path().is_some() {
            // v3: menu-bar mode, launch at login on by default.
            let _ = crate::login::set_enabled(true);
            s.version = 3;
        }
        if crate::login::is_enabled() && crate::login::bundle_path().is_some() {
            let _ = crate::login::set_enabled(true); // refresh the path if the app moved
        }
        let background = std::env::args().any(|a| a == "--background");
        let tray = Tray::new(cc.egui_ctx.clone(), s.enabled);
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
            shot: std::env::var_os("FL_SHOT").map(|p| (p.into(), 0, Default::default())),
            hidden: background && tray.is_some(),
            started: now,
            tray,
            quitting: false,
            login_enabled: crate::login::is_enabled(),
            s,
        };
        if app.s.headphone == CUSTOM && app.custom_curve.is_none() {
            app.s.headphone = 0;
        }
        app.s.headphone =
            if app.s.headphone == CUSTOM { CUSTOM } else { app.s.headphone.min(profiles::HEADPHONES.len() - 1) };
        app.s.target = app.s.target.min(profiles::TARGETS.len() - 1);
        if app.s.engine_on {
            app.start_engine();
        }
        app
    }

    fn fs(&self) -> f64 {
        self.engine.as_ref().map_or(48_000.0, |e| e.info.sample_rate)
    }

    fn headphone(&self) -> (Curve, String, String) {
        if self.s.headphone == CUSTOM
            && let (Some(c), Some(m)) = (&self.custom_curve, &self.s.custom)
        {
            return (c.clone(), m.name.clone(), "Custom measurement (drag & drop)".into());
        }
        let p = &profiles::HEADPHONES[self.s.headphone];
        (p.curve(), p.name.into(), p.note.into())
    }

    fn design_key(&self) -> DesignKey {
        DesignKey {
            headphone: self.s.headphone,
            custom_len: self.s.custom.as_ref().map_or(0, |c| c.csv.len() ^ c.name.len() << 20),
            target: self.s.target,
            params: self.s.params,
            fs: self.fs() as u32,
        }
    }

    fn start_engine(&mut self) {
        self.engine = None; // tear down the previous tap/aggregate first
        self.last_restart = Instant::now();
        let filters = self.design.as_ref().map(|d| d.filters.clone()).unwrap_or_default();
        let (amount, lm) = (self.s.amount, self.s.level_match);
        let uid = self.s.output_uid.clone().filter(|u| self.devices.iter().any(|d| &d.uid == u));
        let frames = std::env::var("FL_BUF").ok().and_then(|v| v.parse().ok()).unwrap_or(self.s.buffer_frames);
        match Engine::start(uid.as_deref(), frames, self.shared.clone(), move |fs| build_eq(&filters, amount, lm, fs).0)
        {
            Ok(e) => {
                self.analyzer.set_rate(e.info.sample_rate);
                self.engine = Some(e);
                self.engine_error = None;
                self.status = Status::Starting;
                self.cb_seen = (self.shared.callbacks.load(Relaxed), Instant::now());
                self.eq_key = None;
            }
            Err(err) => {
                self.engine_error = Some(format!("{err:#}"));
                self.status = Status::Error;
            }
        }
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
        }
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            let name =
                f.path().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Custom".into());
            match f.bytes().map_err(anyhow::Error::msg).and_then(|b| {
                let text = String::from_utf8_lossy(&b).into_owned();
                Curve::parse(&text).map(|c| (c, text))
            }) {
                Ok((curve, csv)) => {
                    self.custom_curve = Some(curve);
                    self.s.custom = Some(CustomCurve { name: name.clone(), csv });
                    self.s.headphone = CUSTOM;
                    self.notify(format!("Loaded measurement “{name}”"));
                }
                Err(e) => self.notify(format!("Could not read {name}: {e}")),
            }
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

    // ------------------------------------------------------------ views --

    fn top_bar(&mut self, ui: &mut Ui) {
        let bar = ui.max_rect();
        let drag =
            ui.interact(bar.expand2(Vec2::new(100.0, 20.0)), ui.scope_id().with("drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.horizontal_centered(|ui| {
            widgets::logo(ui, 30.0);
            ui.add_space(2.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("FLATLINE").font(prop(17.0)).color(TEXT).strong());
                ui.label(RichText::new("headphone calibration · zero-latency EQ").font(prop(11.0)).color(DIM));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let on = self.s.enabled;
                let r = widgets::power_button(ui, on, 42.0).on_hover_text("Correction on / bypass  (Space)");
                if r.clicked() {
                    self.s.enabled = !self.s.enabled;
                }
                ui.allocate_ui_with_layout(Vec2::new(84.0, 40.0), Layout::top_down(Align::Max), |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new("CORRECTION").font(prop(10.0)).color(FAINT));
                    let (t, c) = if on { ("ON", ACCENT) } else { ("BYPASS", AMBER) };
                    ui.label(RichText::new(t).font(mono(15.0)).color(c).strong());
                });
                ui.add_space(16.0);
                let (text, color, pulse) = match self.status {
                    Status::Live => {
                        let info = self.engine.as_ref().map(|e| &e.info);
                        let txt = info.map_or("LIVE".into(), |i| {
                            format!("LIVE · {} · {:.1} kHz", i.device.name, i.sample_rate / 1000.0)
                        });
                        (txt, GREEN, true)
                    }
                    Status::Starting => ("STARTING".into(), AMBER, true),
                    Status::Blocked => ("BLOCKED BY macOS".into(), AMBER, false),
                    Status::Error => ("ERROR".into(), RED, false),
                    Status::Standby => ("STANDBY · system audio untouched".into(), DIM, false),
                };
                widgets::pill(ui, &text, color, pulse);
            });
        });
        let p = ui.painter();
        p.line_segment(
            [Pos2::new(bar.left() - 100.0, bar.bottom() + 11.0), Pos2::new(bar.right() + 30.0, bar.bottom() + 11.0)],
            Stroke::new(1.0, BORDER),
        );
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 10.0;
        ui.add_space(10.0);
        let w = ui.available_width();

        // Headphone.
        widgets::card(ui, |ui| {
            widgets::section_title(ui, "HEADPHONE");
            let (_, name, note) = self.headphone();
            let mut sel = self.s.headphone;
            egui::ComboBox::from_id_salt("hp").width(w - 30.0).selected_text(RichText::new(&name).color(TEXT)).show_ui(
                ui,
                |ui| {
                    for (i, p) in profiles::HEADPHONES.iter().enumerate() {
                        ui.selectable_value(&mut sel, i, p.name);
                    }
                    if let Some(c) = &self.s.custom {
                        ui.selectable_value(&mut sel, CUSTOM, format!("{} (custom)", c.name));
                    }
                },
            );
            self.s.headphone = sel;
            ui.label(RichText::new(note).font(prop(11.0)).color(DIM));
            ui.label(
                RichText::new("Drop a .csv (AutoEq / REW) on the window to calibrate any headphone.")
                    .font(prop(10.5))
                    .color(FAINT),
            );
        });

        // Target.
        widgets::card(ui, |ui| {
            widgets::section_title(ui, "TARGET");
            let mut t = self.s.target;
            egui::ComboBox::from_id_salt("tg")
                .width(w - 30.0)
                .selected_text(RichText::new(profiles::TARGETS[t].name).color(TEXT))
                .show_ui(ui, |ui| {
                    for (i, p) in profiles::TARGETS.iter().enumerate() {
                        ui.selectable_value(&mut t, i, p.name).on_hover_text(p.note);
                    }
                });
            self.s.target = t;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for &(label, bass, tilt) in profiles::PRESETS {
                    let sel =
                        (self.s.params.bass_db - bass).abs() < 0.05 && (self.s.params.tilt_db_oct - tilt).abs() < 0.005;
                    if widgets::chip(ui, label, sel).clicked() {
                        self.s.params.bass_db = bass;
                        self.s.params.tilt_db_oct = tilt;
                    }
                }
            });
            widgets::slider(ui, "Bass", &mut self.s.params.bass_db, -6.0..=12.0, 6.5, 0.5, |v| format!("{v:+.1} dB"));
            widgets::slider(ui, "Tilt", &mut self.s.params.tilt_db_oct, -1.5..=1.5, 0.0, 0.05, |v| {
                format!("{v:+.2} dB/oct")
            });
        });

        // Correction.
        widgets::card(ui, |ui| {
            widgets::section_title(ui, "CORRECTION");
            widgets::slider(ui, "Amount", &mut self.s.amount, 0.0..=1.0, 1.0, 0.01, |v| format!("{:.0} %", v * 100.0));
            widgets::slider(ui, "Max boost", &mut self.s.params.max_boost_db, 0.0..=12.0, 8.0, 0.5, |v| {
                format!("{v:.1} dB")
            });
            let mut bands = self.s.params.n_filters as f64;
            if widgets::slider(ui, "Bands", &mut bands, 4.0..=16.0, 10.0, 1.0, |v| format!("{v:.0}")) {
                self.s.params.n_filters = bands as usize;
            }
            widgets::switch(ui, "Loudness-matched bypass", &mut self.s.level_match);
        });

        // Output.
        widgets::card(ui, |ui| {
            widgets::section_title(ui, "OUTPUT");
            let default_name = self
                .default_uid
                .as_ref()
                .and_then(|u| self.devices.iter().find(|d| &d.uid == u))
                .map_or("—".to_string(), |d| d.name.clone());
            let label = match &self.s.output_uid {
                None => format!("System default · {default_name}"),
                Some(u) => {
                    self.devices.iter().find(|d| &d.uid == u).map_or("(disconnected)".into(), |d| d.name.clone())
                }
            };
            let mut sel = self.s.output_uid.clone();
            egui::ComboBox::from_id_salt("out")
                .width(w - 30.0)
                .selected_text(RichText::new(label).color(TEXT))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut sel, None, format!("System default · {default_name}"));
                    for d in &self.devices {
                        ui.selectable_value(&mut sel, Some(d.uid.clone()), &d.name);
                    }
                });
            if sel != self.s.output_uid {
                self.s.output_uid = sel;
                if self.s.engine_on {
                    self.start_engine();
                }
            }
            ui.label(RichText::new("Buffer (frames)").font(prop(11.5)).color(DIM));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                for b in BUFFER_SIZES {
                    if widgets::chip(ui, &b.to_string(), self.s.buffer_frames == b).clicked()
                        && self.s.buffer_frames != b
                    {
                        self.s.buffer_frames = b;
                        if self.s.engine_on {
                            self.start_engine();
                        }
                    }
                }
            });
            let mut on = self.s.engine_on;
            if widgets::switch(ui, "System-wide processing", &mut on) {
                self.s.engine_on = on;
                if on { self.start_engine() } else { self.stop_engine() }
            }
            let mut login = self.login_enabled;
            if widgets::switch(ui, "Launch at login (menu bar)", &mut login) {
                match crate::login::set_enabled(login) {
                    Ok(()) => self.login_enabled = crate::login::is_enabled(),
                    Err(e) => self.notify(format!("{e:#}")),
                }
            }
            if self.tray.is_some() {
                ui.label(
                    RichText::new("Closing the window keeps Flatline correcting from the menu bar.")
                        .font(prop(10.5))
                        .color(FAINT),
                );
            }
        });

        ui.add_space(4.0);
        ui.label(RichText::new("Space  A/B bypass   ·   double-click a slider to reset").font(prop(10.5)).color(FAINT));
        ui.label(RichText::new("Measurements: oratory1990 via AutoEq").font(prop(10.5)).color(FAINT));
    }

    fn banner(&mut self, ui: &mut Ui) {
        let (title, body, color) = match self.status {
            Status::Blocked => (
                "macOS is blocking system-audio capture",
                "Allow Flatline in System Settings → Privacy & Security → Screen & System Audio Recording → \
                 “System Audio Recording Only”, then press Retry. If Flatline is already enabled there, the grant \
                 belongs to an older build: run `tccutil reset All dev.flatline.app` in Terminal and relaunch. \
                 Your audio plays normally (uncorrected) meanwhile.",
                AMBER,
            ),
            Status::Error => ("Audio engine error", self.engine_error.as_deref().unwrap_or(""), RED),
            _ => return,
        };
        let body = body.to_owned();
        Frame::new()
            .fill(alpha(color, 0.08))
            .stroke(Stroke::new(1.0, alpha(color, 0.4)))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 230.0);
                        ui.label(RichText::new(title).color(color).strong());
                        ui.label(RichText::new(body).font(prop(12.0)).color(DIM));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Retry").clicked() {
                            self.start_engine();
                        }
                        if self.status == Status::Blocked && ui.button("Open Settings").clicked() {
                            let _ = std::process::Command::new("open").arg(SETTINGS_URL).spawn();
                        }
                    });
                });
            });
        ui.add_space(12.0);
    }

    fn tile(ui: &mut Ui, label: &str, value: &str, sub: &str, color: Color32) {
        Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(14, 11))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(RichText::new(label).font(prop(10.0)).color(FAINT).strong());
                ui.label(RichText::new(value).font(mono(18.0)).color(color));
                ui.label(RichText::new(sub).font(prop(10.5)).color(DIM));
            });
    }

    fn main_view(&mut self, ui: &mut Ui) {
        self.banner(ui);
        let avail = ui.available_size();
        let stats_h = 84.0;
        let bottom_h = 158.0;
        let graph_h = (avail.y - stats_h - bottom_h - 24.0 - 64.0).max(220.0);

        // Graph card.
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new("Frequency response").font(prop(15.0)).color(TEXT).strong());
                    let (_, hp, _) = self.headphone();
                    ui.label(
                        RichText::new(format!(
                            "{hp}  →  {}  ·  bass {:+.1} dB",
                            profiles::TARGETS[self.s.target].name,
                            self.s.params.bass_db
                        ))
                        .font(prop(11.5))
                        .color(DIM),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let l = &mut self.s.layers;
                    for (name, color, flag) in [
                        ("Analyzer", alpha(ACCENT, 0.5), &mut l.rta),
                        ("Result", ACCENT, &mut l.result),
                        ("Correction", VIOLET, &mut l.eq),
                        ("Target", TEXT, &mut l.target),
                        ("Headphone", AMBER, &mut l.measured),
                    ] {
                        if legend_chip(ui, name, color, *flag).clicked() {
                            *flag = !*flag;
                        }
                    }
                });
            });
            ui.add_space(6.0);
            let curves = graph::Curves {
                grid: self.design.as_ref().map_or(&[][..], |d| &d.grid[..]),
                measured: &self.disp.measured,
                target: &self.disp.target,
                eq: &self.disp.eq,
                result: &self.disp.result,
            };
            let rta = graph::Rta { freqs: &self.analyzer.freqs, pre: &self.analyzer.db[0], post: &self.analyzer.db[1] };
            let live = self.status == Status::Live;
            let caption = if live { "RTA: shaded = output, grey = input" } else { "" };
            graph::draw(
                ui,
                Vec2::new(ui.available_width(), graph_h),
                &curves,
                live.then_some(&rta),
                &self.s.layers,
                caption,
            );
        });

        ui.add_space(12.0);

        // Stats.
        let (before, after) = self.design.as_ref().map_or((0.0, 0.0), |d| (d.error_before_db, d.error_after_db));
        let info = self.engine.as_ref().map(|e| e.info.clone());
        let offset = self.shared.io_offset.load() as f64;
        let io = info.as_ref().map_or("—".into(), |i| {
            if self.status == Status::Live && offset > 0.0 {
                format!("{:.1} ms", offset / i.sample_rate * 1000.0)
            } else {
                format!("{:.1} ms", i.buffer_frames as f64 / i.sample_rate * 1000.0)
            }
        });
        let io_sub = info.as_ref().map_or("engine off".into(), |i| {
            format!("{} · buffer {} @ {:.0} kHz", i.tap_mode, i.buffer_frames, i.sample_rate / 1000.0)
        });
        ui.columns(5, |cols| {
            Self::tile(
                &mut cols[0],
                "DEVIATION FROM TARGET",
                &format!("{before:.1} → {after:.1} dB"),
                "RMS, 20 Hz – 10 kHz",
                ACCENT,
            );
            Self::tile(
                &mut cols[1],
                "HEADROOM",
                &format!("{:+.1} dB", self.preamp_db),
                "auto preamp, no clipping",
                VIOLET,
            );
            Self::tile(&mut cols[2], "DSP LATENCY", "0 samples", "minimum-phase IIR", GREEN);
            Self::tile(&mut cols[3], "SYSTEM PATH", &io, &io_sub, TEXT);
            Self::tile(&mut cols[4], "DSP LOAD", &format!("{:.2} %", self.load * 100.0), "of the audio deadline", TEXT);
        });

        ui.add_space(12.0);

        // Meters + EQ bands.
        let w = ui.available_width();
        ui.horizontal_top(|ui| {
            ui.allocate_ui(Vec2::new(w * 0.36, bottom_h), |ui| {
                widgets::card(ui, |ui| {
                    ui.set_min_height(bottom_h - 28.0);
                    ui.horizontal(|ui| {
                        widgets::section_title(ui, "LEVELS");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let clip = self.clip_until.is_some_and(|t| Instant::now() < t);
                            let c = if clip { RED } else { BORDER };
                            ui.label(RichText::new("● CLIP").font(mono(10.5)).color(c));
                        });
                    });
                    ui.spacing_mut().item_spacing.y = 5.0;
                    for (i, l) in ["IN  L", "IN  R", "OUT L", "OUT R"].iter().enumerate() {
                        widgets::meter(ui, l, self.meters.level[i], self.meters.hold[i]);
                    }
                    let scale = ui.allocate_exact_size(Vec2::new(ui.available_width(), 12.0), Sense::hover()).0;
                    let bar_l = scale.left() + 46.0;
                    for db in [-60, -48, -36, -24, -12, -6, 0] {
                        let x = bar_l + (db as f32 + 60.0) / 60.0 * (scale.right() - bar_l);
                        ui.painter().text(
                            Pos2::new(x, scale.top()),
                            Align2::CENTER_TOP,
                            db.to_string(),
                            mono(9.0),
                            FAINT,
                        );
                    }
                });
            });
            ui.allocate_ui(Vec2::new(ui.available_width(), bottom_h), |ui| {
                widgets::card(ui, |ui| {
                    ui.set_min_height(bottom_h - 28.0);
                    ui.horizontal(|ui| {
                        widgets::section_title(ui, "EQ BANDS");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .button(RichText::new("Copy EQ").font(prop(12.0)))
                                .on_hover_text("Equalizer APO / AutoEq ParametricEQ format")
                                .clicked()
                            {
                                ui.ctx().copy_text(self.eq_text());
                                self.notify("EQ copied to clipboard");
                            }
                        });
                    });
                    self.band_strip(ui);
                });
            });
        });
    }

    fn band_strip(&self, ui: &mut Ui) {
        let Some(d) = &self.design else { return };
        let bands = d.scaled_filters(self.s.amount);
        ScrollArea::horizontal().id_salt("bands").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for f in &bands {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(64.0, 96.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(
                        rect,
                        CornerRadius::same(8),
                        if resp.hovered() { CARD_HI } else { Color32::from_rgb(20, 25, 33) },
                    );
                    let kind_col = match f.kind {
                        FilterKind::Peak => DIM,
                        _ => VIOLET,
                    };
                    p.text(
                        rect.center_top() + Vec2::new(0.0, 6.0),
                        Align2::CENTER_TOP,
                        f.kind.short(),
                        mono(10.0),
                        kind_col,
                    );
                    p.text(
                        rect.center_top() + Vec2::new(0.0, 20.0),
                        Align2::CENTER_TOP,
                        format!("{} Hz", graph::fmt_freq(f.freq)),
                        mono(11.5),
                        TEXT,
                    );
                    // Gain bar (±12 dB).
                    let mid = rect.top() + 56.0;
                    let h = (f.gain_db as f32 / 12.0).clamp(-1.0, 1.0) * 14.0;
                    let col = if f.gain_db >= 0.0 { AMBER } else { ACCENT };
                    let bar = Rect::from_min_max(
                        Pos2::new(rect.center().x - 10.0, mid - h.max(0.0)),
                        Pos2::new(rect.center().x + 10.0, mid - h.min(0.0)),
                    );
                    p.line_segment(
                        [Pos2::new(rect.left() + 8.0, mid), Pos2::new(rect.right() - 8.0, mid)],
                        Stroke::new(1.0, BORDER),
                    );
                    p.rect_filled(bar, CornerRadius::same(2), alpha(col, 0.8));
                    p.text(
                        rect.center_bottom() - Vec2::new(0.0, 19.0),
                        Align2::CENTER_BOTTOM,
                        format!("{:+.1}", f.gain_db),
                        mono(11.0),
                        col,
                    );
                    p.text(
                        rect.center_bottom() - Vec2::new(0.0, 6.0),
                        Align2::CENTER_BOTTOM,
                        format!("Q {:.2}", f.q),
                        mono(9.5),
                        FAINT,
                    );
                }
            });
        });
    }

    fn overlays(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop")));
            p.rect_filled(screen, CornerRadius::ZERO, alpha(BG, 0.85));
            let r = screen.shrink(40.0);
            for s in egui::Shape::dashed_line(
                &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()],
                Stroke::new(2.0, ACCENT),
                10.0,
                8.0,
            ) {
                p.add(s);
            }
            p.text(screen.center(), Align2::CENTER_CENTER, "Drop a frequency-response CSV", prop(22.0), ACCENT);
        }
        if let Some((msg, t)) = &self.toast {
            let age = t.elapsed().as_secs_f32();
            if age > 2.6 {
                self.toast = None;
            } else {
                let a = (1.0 - ((age - 2.2) / 0.4).clamp(0.0, 1.0)) * (age / 0.15).min(1.0);
                let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("toast")));
                let galley = p.layout_no_wrap(msg.clone(), prop(13.0), alpha(TEXT, a));
                let size = galley.size() + Vec2::new(32.0, 18.0);
                let r = Rect::from_center_size(Pos2::new(screen.center().x, screen.bottom() - 40.0), size);
                p.rect_filled(r, CornerRadius::same(10), alpha(CARD_HI, 0.97 * a));
                p.rect_stroke(
                    r,
                    CornerRadius::same(10),
                    Stroke::new(1.0, alpha(ACCENT, 0.5 * a)),
                    egui::StrokeKind::Inside,
                );
                p.galley(r.center() - galley.size() / 2.0, galley, TEXT);
                ctx.request_repaint();
            }
        }
    }
}

fn legend_chip(ui: &mut Ui, name: &str, color: Color32, on: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(name.to_owned(), prop(11.5), TEXT);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 26.0, 22.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(11), CARD_HI);
    }
    p.circle_filled(Pos2::new(rect.left() + 10.0, rect.center().y), 4.0, if on { color } else { alpha(color, 0.2) });
    p.text(
        Pos2::new(rect.left() + 19.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        prop(11.5),
        if on { TEXT } else { FAINT },
    );
    resp
}

impl eframe::App for FlatlineApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;

        // Correction design (worker thread).
        let key = self.design_key();
        if self.requested.as_ref() != Some(&key) {
            let (hp, _, _) = self.headphone();
            let tg = profiles::TARGETS[self.s.target].curve();
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
            if let Some(e) = &self.engine {
                let cb = self.shared.callbacks.load(Relaxed);
                if cb != self.cb_seen.0 {
                    self.cb_seen = (cb, now);
                    self.status = Status::Live;
                } else if now - self.cb_seen.1 > Duration::from_millis(1500) {
                    self.status = Status::Blocked;
                }
                let retry_blocked = self.status == Status::Blocked && now - self.last_restart > Duration::from_secs(3);
                let want = self
                    .s
                    .output_uid
                    .clone()
                    .filter(|u| self.devices.iter().any(|d| &d.uid == u))
                    .or(self.default_uid.clone());
                let moved = want.as_deref().is_some_and(|w| w != e.info.device.uid);
                let rate_changed = e.current_sample_rate().is_some_and(|r| (r - e.info.sample_rate).abs() > 1.0);
                if retry_blocked
                    || ((moved || rate_changed || !e.device_alive())
                        && now - self.last_restart > Duration::from_millis(1500))
                {
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
        self.meters
            .update([sh.peak_in[0].take(), sh.peak_in[1].take(), sh.peak_out[0].take(), sh.peak_out[1].take()], dt);
        let load = sh.load.take();
        self.load = if load > self.load { load } else { self.load + (load - self.load) * 0.05 };
        let clips = sh.clips.load(Relaxed);
        if clips != self.clips_seen {
            self.clips_seen = clips;
            self.clip_until = Some(now + Duration::from_millis(1500));
        }

        if ctx.input(|i| i.key_pressed(Key::Space)) && !ctx.egui_wants_keyboard_input() {
            self.s.enabled = !self.s.enabled;
        }
        self.handle_drops(ctx);

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

        ctx.request_repaint_after(if self.engine.is_some() && !self.hidden {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(250)
        });
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("top")
            .exact_size(66.0)
            .show_separator_line(false)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin { left: 86, right: 18, top: 12, bottom: 10 }))
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::left("side")
            .exact_size(316.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin { left: 14, right: 10, top: 0, bottom: 10 }))
            .show(ui, |ui| {
                ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| self.sidebar(ui));
            });
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG).inner_margin(Margin::same(16)))
            .show(ui, |ui| self.main_view(ui));
        self.overlays(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if !self.demo {
            eframe::set_value(storage, eframe::APP_KEY, &self.s);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.engine = None;
    }
}
