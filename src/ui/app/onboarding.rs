//! First-run setup and the headphone picker.
//!
//! A1 Welcome → A2 Allow system audio → A3 Choose your headphones (search
//! the AutoEq catalog) → A4 Pick your sound. A3 alone is also the
//! "Add headphones…" picker opened from the switcher.

use super::super::graph::{self, Layers};
use super::super::theme::*;
use super::super::widgets::{self, Icon, paint_glass};
use super::views::{primary_button, secondary_button};
use super::{CUSTOM_KEY, DEFAULT_KEY, FlatlineApp, Status};
use crate::catalog::{Catalog, Entry, Form};
use crate::dsp::fit::{self, DesignParams};
use crate::profiles;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Layout, Mesh, Pos2, Rect, RichText, ScrollArea, Sense, Shape, Stroke,
    StrokeKind, TextEdit, Ui, UiBuilder, Vec2,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A sound preset: a bass shelf and a tilt on top of the rig's target.
pub(super) struct Preset {
    /// Label.
    pub name: &'static str,
    /// Bass shelf at 105 Hz, dB.
    pub bass: f64,
    /// Tilt, dB per octave.
    pub tilt: f64,
    /// One-line description.
    pub blurb: &'static str,
}

/// The four presets for a target whose default bass shelf is `default_bass`
/// (AutoEq's value for the measurement's target: 6 dB over-ear, 8 dB in-ear).
pub(super) fn presets(default_bass: f64) -> [Preset; 4] {
    let half = (default_bass).round() / 2.0;
    [
        Preset { name: "Flat", bass: 0.0, tilt: 0.0, blurb: "Neutral reference" },
        Preset { name: "Studio", bass: half, tilt: 0.0, blurb: "A touch of low end" },
        Preset { name: "Harman", bass: default_bass, tilt: 0.0, blurb: "The research-backed favourite" },
        Preset { name: "Warm", bass: default_bass + 1.5, tilt: -0.3, blurb: "Rich and relaxed" },
    ]
}

/// Setup step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Step {
    Welcome,
    Access,
    Pick,
    Sound,
}

/// Fitted preview of one catalog entry (A3 right-hand card).
struct Preview {
    grid: Vec<f64>,
    measured: Vec<f32>,
    target: Vec<f32>,
    before: f64,
    after: f64,
}

/// State of the setup screens.
pub(super) struct Onboarding {
    step: Step,
    /// Opened from the switcher: only the picker, then back.
    add_only: bool,
    query: String,
    form: Option<Form>,
    selected: String,
    results: Vec<&'static Entry>,
    results_for: Option<(String, Option<Form>)>,
    sparks: HashMap<String, Vec<f32>>,
    previews: Arc<Mutex<HashMap<String, Preview>>>,
    preview_pending: Option<String>,
    requested_access: bool,
    preset: usize,
    sound_curves: Option<(String, Vec<f64>, Vec<Vec<f32>>)>,
    browse: Arc<Mutex<Option<std::path::PathBuf>>>,
    focus_search: bool,
}

enum Act {
    Stay,
    Go(Step),
    Finish,
}

impl Onboarding {
    fn blank(step: Step, add_only: bool) -> Self {
        Self {
            step,
            add_only,
            query: String::new(),
            form: None,
            selected: String::new(),
            results: Vec::new(),
            results_for: None,
            sparks: HashMap::new(),
            previews: Default::default(),
            preview_pending: None,
            requested_access: false,
            preset: 2,
            sound_curves: None,
            browse: Default::default(),
            focus_search: true,
        }
    }

    /// First-run setup (`forced` picks a screen in demo mode).
    pub(super) fn new(forced: &Option<String>) -> Self {
        let step = match forced.as_deref() {
            Some("access") => Step::Access,
            Some("pick") => Step::Pick,
            Some("sound") => Step::Sound,
            _ => Step::Welcome,
        };
        let mut o = Self::blank(step, false);
        if step != Step::Welcome && step != Step::Access {
            o.selected = DEFAULT_KEY.into();
            o.query = "DT 770".into();
        }
        o
    }

    /// The picker alone ("Add headphones…").
    pub(super) fn picker() -> Self {
        Self::blank(Step::Pick, true)
    }

    /// Full-colour light on the setup screens.
    pub(super) fn is_setup(&self) -> bool {
        !self.add_only
    }

    /// A measurement was dropped or browsed: it becomes the selection.
    pub(super) fn custom_loaded(&mut self) {
        self.selected = CUSTOM_KEY.into();
    }
}

fn centered(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Center)), add);
}

fn radio(p: &egui::Painter, c: Pos2, on: bool, dark: bool) {
    let ink = if dark { NIGHT } else { MILK };
    p.circle_stroke(c, 9.0, Stroke::new(1.5, alpha(ink, if on { 1.0 } else { 0.45 })));
    if on {
        p.circle_filled(c, 5.0, ink);
    }
}

fn badge(p: &egui::Painter, pos: Pos2, text: &str) -> Rect {
    let g = p.layout_no_wrap(text.to_owned(), medium(12.0), PULSE);
    let r = Rect::from_min_size(pos, g.size() + Vec2::new(18.0, 8.0));
    p.rect_filled(r, CornerRadius::same(8), alpha(PULSE, 0.16));
    p.galley(r.min + Vec2::new(9.0, 4.0), g, PULSE);
    r
}

fn source_tag(p: &egui::Painter, right: Pos2, text: &str, dark: bool) -> Rect {
    let ink = if dark { NIGHT } else { MILK };
    let g = p.layout_no_wrap(text.to_owned(), body(12.0), alpha(ink, 0.8));
    let size = g.size() + Vec2::new(20.0, 8.0);
    let r = Rect::from_min_size(Pos2::new(right.x - size.x, right.y - size.y / 2.0), size);
    p.rect_stroke(r, CornerRadius::same(12), Stroke::new(1.0, alpha(ink, 0.3)), StrokeKind::Inside);
    p.galley(r.min + Vec2::new(10.0, 4.0), g, ink);
    r
}

fn sparkline(p: &egui::Painter, rect: Rect, db: &[f32], color: Color32) {
    if db.len() < 2 {
        return;
    }
    // Clamp first: some measurements fall far below (Bluetooth low-pass).
    let db: Vec<f32> = db.iter().map(|v| v.clamp(-30.0, 30.0)).collect();
    let mean = db.iter().sum::<f32>() / db.len() as f32;
    let pts: Vec<Pos2> = db
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = rect.left() + rect.width() * i as f32 / (db.len() - 1) as f32;
            let y = rect.center().y - ((v - mean) / 14.0).clamp(-1.0, 1.0) * rect.height() / 2.0;
            Pos2::new(x, y)
        })
        .collect();
    p.line(pts, Stroke::new(1.5, color));
}

impl FlatlineApp {
    /// Whether the setup screens are showing (full-colour light).
    pub(super) fn in_setup(&self) -> bool {
        self.onb.as_ref().is_some_and(Onboarding::is_setup)
    }

    /// The setup screens or the picker, full window.
    pub(super) fn onboarding_view(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        let time = ui.input(|i| i.time);
        self.aura.paint(ui.painter(), full, time);
        let Some(mut o) = self.onb.take() else { return };
        // A browsed CSV arrived.
        let browsed = o.browse.lock().ok().and_then(|mut b| b.take());
        let act = match o.step {
            Step::Welcome => self.onb_welcome(ui, full),
            Step::Access => self.onb_access(ui, full, &mut o, time),
            Step::Pick => self.onb_pick(ui, full, &mut o),
            Step::Sound => self.onb_sound(ui, full, &mut o),
        };
        let blocked = o.step == Step::Access && o.requested_access && self.status == Status::Blocked;
        match act {
            Act::Stay => self.onb = Some(o),
            Act::Go(step) => {
                o.step = step;
                o.focus_search = true;
                self.onb = Some(o);
            }
            Act::Finish => {}
        }
        if let Some(path) = browsed {
            let name = path.file_stem().map_or_else(|| "Custom".into(), |s| s.to_string_lossy().into_owned());
            let text =
                std::fs::read(&path).map(|b| String::from_utf8_lossy(&b).into_owned()).map_err(anyhow::Error::from);
            self.load_measurement(name, text);
        }
        if blocked {
            self.blocked_overlay(ui, full);
        }
        self.overlays(ui.ctx());
    }

    /// Logo and step indicator (steps hidden for the picker alone).
    fn onb_header(&self, ui: &mut Ui, full: Rect, o: &Onboarding) {
        let rect = Rect::from_min_size(full.min + Vec2::new(32.0, 14.0), Vec2::new(full.width() - 64.0, 52.0));
        let drag = ui.interact(rect, ui.scope_id().with("onb-drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.scope_builder(
            UiBuilder::new().max_rect(Rect::from_min_size(rect.min + Vec2::new(52.0, 10.0), Vec2::new(260.0, 34.0))),
            |ui| {
                widgets::logo(ui, 25.0);
            },
        );
        if o.add_only {
            return;
        }
        let p = ui.painter();
        let steps = [("Access", Step::Access), ("Headphones", Step::Pick), ("Sound", Step::Sound)];
        let order = |s: Step| match s {
            Step::Welcome => 0,
            Step::Access => 1,
            Step::Pick => 2,
            Step::Sound => 3,
        };
        let galleys: Vec<_> = steps.iter().map(|(t, _)| p.layout_no_wrap((*t).into(), body(13.5), MILK)).collect();
        let w: f32 = galleys.iter().map(|g| g.size().x + 34.0).sum::<f32>() + 2.0 * 34.0 + 24.0;
        let pill = Rect::from_center_size(Pos2::new(rect.center().x, rect.center().y), Vec2::new(w, 40.0));
        p.rect_filled(pill, CornerRadius::same(20), glass_fill());
        p.rect_stroke(pill, CornerRadius::same(20), Stroke::new(1.0, glass_edge()), StrokeKind::Inside);
        let mut x = pill.left() + 12.0;
        for (i, ((_, step), g)) in steps.iter().zip(galleys).enumerate() {
            if i > 0 {
                p.line_segment(
                    [Pos2::new(x + 4.0, pill.center().y), Pos2::new(x + 24.0, pill.center().y)],
                    Stroke::new(1.0, alpha(MILK, 0.3)),
                );
                x += 34.0;
            }
            let c = Pos2::new(x + 10.0, pill.center().y);
            let (done, now) = (order(o.step) > order(*step), o.step == *step);
            if done {
                p.circle_filled(c, 10.0, SOLEIL);
                widgets::draw_icon(p, Icon::Check, c, 14.0, NIGHT);
            } else if now {
                p.circle_filled(c, 10.0, MILK);
                p.text(c, Align2::CENTER_CENTER, (i + 1).to_string(), medium(12.0), NIGHT);
            } else {
                p.circle_stroke(c, 9.5, Stroke::new(1.0, alpha(MILK, 0.5)));
                p.text(c, Align2::CENTER_CENTER, (i + 1).to_string(), medium(12.0), MIST);
            }
            let color = if now || done { MILK } else { MIST };
            let gw = g.size().x;
            p.galley(Pos2::new(x + 26.0, pill.center().y - g.size().y / 2.0), g, color);
            x += 26.0 + gw;
        }
    }

    // ------------------------------------------------------- A1 welcome --

    fn onb_welcome(&mut self, ui: &mut Ui, full: Rect) -> Act {
        let drag = ui.interact(full, ui.scope_id().with("welcome-drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let c = full.center();
        let logo_rect = Rect::from_center_size(Pos2::new(c.x, c.y - 178.0), Vec2::new(full.width(), 90.0));
        let mut logo = Rect::NOTHING;
        centered(ui, logo_rect, |ui| logo = widgets::logo(ui, 64.0).rect);
        // The horizon carries on across the window.
        let y = logo.top() + logo.height() * 0.80;
        let p = ui.painter();
        for (a, b) in [(full.left() + 24.0, logo.left() - 26.0), (logo.right() + 26.0, full.right() - 24.0)] {
            let n = 24;
            for k in 0..n {
                let (t0, t1) = (k as f32 / n as f32, (k + 1) as f32 / n as f32);
                let fade = if a < logo.left() { t1 } else { 1.0 - t0 };
                p.line_segment(
                    [Pos2::new(a + (b - a) * t0, y), Pos2::new(a + (b - a) * t1, y)],
                    Stroke::new(1.0, alpha(MILK, 0.22 * fade)),
                );
            }
        }
        let text = Rect::from_center_size(Pos2::new(c.x, c.y + 10.0), Vec2::new(full.width() - 80.0, 200.0));
        let mut next = false;
        centered(ui, text, |ui| {
            ui.spacing_mut().item_spacing.y = 18.0;
            let size = (full.width() / 24.0).clamp(36.0, 54.0);
            let g = ui.painter().layout_job(widgets::tight(
                "Your headphones, without the colour.",
                title(size),
                MILK,
                -0.03,
            ));
            let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
            ui.painter().galley(r.min, g, MILK);
            ui.label(
                RichText::new("System-wide headphone calibration · 0 samples of added latency")
                    .font(light(18.0))
                    .color(MIST),
            );
            ui.add_space(14.0);
            next = primary_button(ui, "Get started").clicked();
        });
        ui.painter().text(
            Pos2::new(c.x, full.bottom() - 40.0),
            Align2::CENTER_BOTTOM,
            "Takes about a minute · You can change everything later",
            body(13.0),
            MIST2,
        );
        if next || ui.input(|i| i.key_pressed(egui::Key::Enter)) { Act::Go(Step::Access) } else { Act::Stay }
    }

    // ------------------------------------------------------- A2 access --

    fn onb_access(&mut self, ui: &mut Ui, full: Rect, o: &mut Onboarding, time: f64) -> Act {
        self.onb_header(ui, full, o);
        if o.requested_access && self.status == Status::Live {
            return Act::Go(Step::Pick);
        }
        let (w, h) = (full.width(), full.height());
        let card = Rect::from_min_size(
            Pos2::new(full.left() + (w * 0.11).max(48.0), full.top() + h * 0.25),
            Vec2::new((w * 0.43).clamp(460.0, 580.0), 420.0),
        );
        paint_glass(ui.painter(), card, 28);
        let ic = card.left_top() + Vec2::new(54.0, 56.0);
        let p = ui.painter();
        p.circle_filled(ic, 24.0, alpha(Color32::WHITE, 0.08));
        p.circle_stroke(ic, 24.0, Stroke::new(1.0, glass_edge()));
        widgets::draw_icon(p, Icon::Ear, ic, 26.0, MILK);
        let inner = Rect::from_min_max(card.min + Vec2::new(30.0, 98.0), card.max - Vec2::new(30.0, 24.0));
        let mut allow = false;
        let waiting = o.requested_access && self.status == Status::Starting;
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
            ui.spacing_mut().item_spacing.y = 12.0;
            ui.label(RichText::new("Flatline needs to hear what your Mac plays").font(title(29.0)).color(MILK));
            ui.add_space(4.0);
            for (n, line) in [
                "Flatline captures the audio going to your headphones",
                "It corrects it in real time, with zero added latency",
                "Nothing is recorded or leaves your Mac",
            ]
            .iter()
            .enumerate()
            {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
                    ui.painter().circle_stroke(r.center(), 12.0, Stroke::new(1.0, alpha(Color32::WHITE, 0.3)));
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, (n + 1).to_string(), medium(12.5), MILK);
                    ui.label(RichText::new(*line).font(body(15.0)).color(MILK));
                });
            }
            ui.add_space(8.0);
            if waiting {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(26.0, 46.0), Sense::hover());
                    let a = time as f32 * 4.0;
                    let pts: Vec<Pos2> = (0..=20)
                        .map(|i| {
                            let t = a + i as f32 / 20.0 * 4.5;
                            r.center() + Vec2::new(t.cos(), t.sin()) * 9.0
                        })
                        .collect();
                    ui.painter().line(pts, Stroke::new(2.0, MILK));
                    ui.label(RichText::new("Waiting for macOS…").font(medium(15.0)).color(MILK));
                });
                ui.ctx().request_repaint();
            } else {
                allow = primary_button(ui, if o.requested_access { "Try again" } else { "Allow access" }).clicked();
            }
            ui.label(
                RichText::new("You can change this later in System Settings › Privacy & Security.")
                    .font(body(12.5))
                    .color(MIST2),
            );
        });
        if allow {
            o.requested_access = true;
            self.s.engine_on = true;
            self.blocked_retries = 0;
            self.start_engine();
        }

        // What macOS will show (illustration, not interactive).
        let d = Rect::from_min_size(
            Pos2::new(card.right() + (w * 0.07).max(40.0), card.top() + 84.0),
            Vec2::new(236.0, 200.0),
        );
        if d.right() < full.right() - 24.0 {
            let p = ui.painter();
            p.text(
                d.left_top() - Vec2::new(0.0, 14.0),
                Align2::LEFT_BOTTOM,
                "THEN MACOS ASKS, ONCE",
                medium(11.0),
                MIST,
            );
            p.rect_filled(d, CornerRadius::same(14), Color32::from_rgb(40, 38, 43));
            p.rect_stroke(d, CornerRadius::same(14), Stroke::new(1.0, alpha(Color32::WHITE, 0.14)), StrokeKind::Inside);
            let icon = Rect::from_center_size(Pos2::new(d.center().x, d.top() + 40.0), Vec2::splat(46.0));
            p.rect_filled(icon, CornerRadius::same(11), NIGHT);
            p.rect_stroke(
                icon,
                CornerRadius::same(11),
                Stroke::new(1.0, alpha(Color32::WHITE, 0.12)),
                StrokeKind::Inside,
            );
            widgets::paint_symbol(p, icon.center() - Vec2::new(0.0, 2.0), 7.5, None);
            let t =
                p.layout("“Flatline” would like to record system audio.".into(), medium(12.5), MILK, d.width() - 30.0);
            let ty = icon.bottom() + 10.0;
            let th = t.size().y;
            p.galley(Pos2::new(d.center().x - t.size().x / 2.0, ty), t, MILK);
            let s = p.layout(
                "Flatline uses it to correct your headphones in real time. Nothing is recorded.".into(),
                body(10.5),
                MIST,
                d.width() - 34.0,
            );
            p.galley(Pos2::new(d.center().x - s.size().x / 2.0, ty + th + 6.0), s, MIST);
            let by = d.bottom() - 34.0;
            let bw = (d.width() - 36.0) / 2.0;
            for (i, (label, fill)) in
                [("Don’t Allow", Color32::from_rgb(78, 76, 82)), ("Allow", Color32::from_rgb(10, 132, 255))]
                    .iter()
                    .enumerate()
            {
                let r =
                    Rect::from_min_size(Pos2::new(d.left() + 12.0 + i as f32 * (bw + 12.0), by), Vec2::new(bw, 22.0));
                p.rect_filled(r, CornerRadius::same(6), *fill);
                p.text(r.center(), Align2::CENTER_CENTER, *label, body(11.5), Color32::WHITE);
            }
        }
        Act::Stay
    }

    // ------------------------------------------------- A3 headphones --

    fn onb_pick(&mut self, ui: &mut Ui, full: Rect, o: &mut Onboarding) -> Act {
        self.onb_header(ui, full, o);
        let mx = 32.0;
        let top = full.top() + 76.0;
        let content =
            Rect::from_min_max(Pos2::new(full.left() + mx, top), Pos2::new(full.right() - mx, full.bottom() - 26.0));
        let p = ui.painter();
        let title_text = if o.add_only { "Add headphones" } else { "Choose your headphones" };
        let tg = p.layout_job(widgets::tight(title_text, display(48.0), MILK, -0.04));
        let title_h = tg.size().y;
        p.galley(content.min, tg, MILK);
        let info_y = content.top() + title_h + 4.0;
        widgets::draw_icon(p, Icon::Warning, Pos2::new(content.left() + 8.0, info_y + 9.0), 14.0, MIST);
        p.text(
            Pos2::new(content.left() + 24.0, info_y),
            Align2::LEFT_TOP,
            "Flatline can’t detect your headphones: pick yours once, it remembers.",
            body(14.0),
            MIST,
        );

        let left_w = (content.width() * 0.6).max(520.0);
        let col_top = info_y + 34.0;
        let footer_h = 56.0;
        let left = Rect::from_min_max(
            Pos2::new(content.left(), col_top),
            Pos2::new(content.left() + left_w, content.bottom() - footer_h),
        );
        let right = Rect::from_min_max(
            Pos2::new(left.right() + 24.0, col_top),
            Pos2::new(content.right(), content.bottom() - footer_h),
        );

        // Search box.
        let search = Rect::from_min_size(left.min, Vec2::new(left.width(), 52.0));
        let p = ui.painter();
        p.rect_filled(search, CornerRadius::same(26), alpha(Color32::WHITE, 0.07));
        p.rect_stroke(
            search,
            CornerRadius::same(26),
            Stroke::new(1.0, alpha(Color32::WHITE, 0.24)),
            StrokeKind::Inside,
        );
        widgets::draw_icon(p, Icon::Search, Pos2::new(search.left() + 26.0, search.center().y), 20.0, MILK);
        let n = Catalog::get().len() / 100 * 100;
        let hint = format!("{},{:03}+ measurements from AutoEq", n / 1000, n % 1000);
        p.text(Pos2::new(search.right() - 60.0, search.center().y), Align2::RIGHT_CENTER, &hint, body(12.5), MIST2);
        let edit = Rect::from_min_max(
            Pos2::new(search.left() + 48.0, search.top() + 13.0),
            Pos2::new(search.right() - 230.0, search.bottom() - 11.0),
        );
        let resp = ui.put(
            edit,
            TextEdit::singleline(&mut o.query)
                .frame(egui::Frame::NONE)
                .font(body(17.0))
                .text_color(MILK)
                .hint_text(RichText::new("Search a brand or model, e.g. DT 770").font(body(17.0)).color(MIST2)),
        );
        if o.focus_search {
            resp.request_focus();
            o.focus_search = false;
        }
        if !o.query.is_empty() {
            let c = Pos2::new(search.right() - 30.0, search.center().y);
            let r =
                ui.interact(Rect::from_center_size(c, Vec2::splat(26.0)), ui.scope_id().with("clear"), Sense::click());
            ui.painter().circle_filled(c, 13.0, alpha(Color32::WHITE, if r.hovered() { 0.2 } else { 0.1 }));
            widgets::draw_icon(ui.painter(), Icon::Close, c, 14.0, MILK);
            if r.clicked() {
                o.query.clear();
                resp.request_focus();
            }
        }

        // Form filters.
        let filters =
            Rect::from_min_size(Pos2::new(left.left(), search.bottom() + 14.0), Vec2::new(left.width(), 36.0));
        ui.scope_builder(UiBuilder::new().max_rect(filters).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if widgets::pill(ui, "All", o.form.is_none()).clicked() {
                o.form = None;
            }
            for f in Form::ALL {
                if widgets::pill(ui, f.label(), o.form == Some(f)).clicked() {
                    o.form = Some(f);
                }
            }
        });

        // Results.
        let want = (o.query.clone(), o.form);
        if o.results_for.as_ref() != Some(&want) {
            o.results = Catalog::get().search(&o.query, o.form, 80);
            o.results_for = Some(want);
        }
        let not_listed_h = 66.0;
        let list = Rect::from_min_max(
            Pos2::new(left.left(), filters.bottom() + 14.0),
            Pos2::new(left.right(), left.bottom() - not_listed_h - 12.0),
        );
        let mut chosen: Option<String> = None;
        let mut confirm = false;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                let n = o.results.len();
                if n == 0 {
                    ui.add_space(30.0);
                    ui.label(
                        RichText::new("No match. Try fewer words, or drop a measurement below.")
                            .font(body(15.0))
                            .color(MIST),
                    );
                }
                let mut brand = "";
                let mut recommended_shown = false;
                for (i, e) in o.results.clone().into_iter().enumerate() {
                    if e.brand != brand {
                        brand = e.brand;
                        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
                        ui.painter().text(
                            r.left_bottom(),
                            Align2::LEFT_BOTTOM,
                            brand.to_uppercase(),
                            medium(11.5),
                            MIST,
                        );
                        if i == 0 && !o.query.is_empty() {
                            let label = if n >= 80 {
                                "80+ matches".to_owned()
                            } else {
                                format!("{n} matches for “{}”", o.query.trim())
                            };
                            ui.painter().text(r.right_bottom(), Align2::RIGHT_BOTTOM, label, body(12.0), MIST2);
                        }
                    }
                    let key = e.key();
                    let selected = o.selected == key;
                    let (r, row) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::click());
                    let p = ui.painter();
                    let (fill, ink) = if selected {
                        (MILK, NIGHT)
                    } else {
                        (alpha(Color32::WHITE, if row.hovered() { 0.10 } else { 0.05 }), MILK)
                    };
                    p.rect_filled(r, CornerRadius::same(16), fill);
                    if !selected {
                        p.rect_stroke(
                            r,
                            CornerRadius::same(16),
                            Stroke::new(1.0, alpha(Color32::WHITE, 0.08)),
                            StrokeKind::Inside,
                        );
                    }
                    radio(p, Pos2::new(r.left() + 24.0, r.center().y), selected, selected);
                    p.text(Pos2::new(r.left() + 46.0, r.top() + 11.0), Align2::LEFT_TOP, e.model, medium(15.0), ink);
                    let sub = e.variant.unwrap_or("Stock");
                    p.text(
                        Pos2::new(r.left() + 46.0, r.top() + 32.0),
                        Align2::LEFT_TOP,
                        sub,
                        body(12.5),
                        alpha(ink, 0.65),
                    );
                    let tag = source_tag(p, Pos2::new(r.right() - 14.0, r.center().y), e.source, selected);
                    let spark_r = Rect::from_min_max(
                        Pos2::new(tag.left() - 96.0, r.top() + 14.0),
                        Pos2::new(tag.left() - 14.0, r.bottom() - 14.0),
                    );
                    let spark = o.sparks.entry(key.clone()).or_insert_with(|| Catalog::get().sparkline(e, 40));
                    sparkline(p, spark_r, spark, if selected { PULSE } else { alpha(MILK, 0.7) });
                    if !recommended_shown && e.rank == 0 && e.variant.is_none() && !o.query.is_empty() {
                        recommended_shown = true;
                        let b = p.layout_no_wrap("Recommended".into(), medium(12.0), PULSE).size().x + 18.0;
                        badge(p, Pos2::new(spark_r.left() - b - 12.0, r.center().y - 12.0), "Recommended");
                    }
                    if row.clicked() {
                        chosen = Some(key.clone());
                    }
                    if row.double_clicked() {
                        chosen = Some(key);
                        confirm = true;
                    }
                }
            });
        });
        if let Some(k) = chosen {
            o.selected = k;
        }

        // Not listed: drop or browse a CSV.
        let nl = Rect::from_min_max(Pos2::new(left.left(), left.bottom() - not_listed_h), left.max);
        let p = ui.painter();
        for s in Shape::dashed_line(
            &[nl.left_top(), nl.right_top(), nl.right_bottom(), nl.left_bottom(), nl.left_top()],
            Stroke::new(1.0, alpha(Color32::WHITE, 0.3)),
            6.0,
            5.0,
        ) {
            p.add(s);
        }
        let fc = Pos2::new(nl.left() + 30.0, nl.center().y);
        p.circle_stroke(fc, 16.0, Stroke::new(1.0, alpha(Color32::WHITE, 0.25)));
        widgets::draw_icon(p, Icon::File, fc, 18.0, MILK);
        p.text(
            Pos2::new(nl.left() + 58.0, nl.top() + 14.0),
            Align2::LEFT_TOP,
            "Not listed? Drop a frequency-response CSV (AutoEq, REW)",
            medium(14.0),
            MILK,
        );
        p.text(
            Pos2::new(nl.left() + 58.0, nl.top() + 35.0),
            Align2::LEFT_TOP,
            "Flatline fits the correction from your own measurement.",
            body(12.5),
            MIST,
        );
        let br = Rect::from_min_max(
            Pos2::new(nl.right() - 120.0, nl.top() + 10.0),
            Pos2::new(nl.right() - 12.0, nl.bottom() - 10.0),
        );
        if ui.scope_builder(UiBuilder::new().max_rect(br), |ui| secondary_button(ui, "Browse…").clicked()).inner {
            let slot = o.browse.clone();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let out = std::process::Command::new("osascript")
                    .args(["-e", "POSIX path of (choose file with prompt \"Choose a frequency-response CSV\")"])
                    .output();
                if let Ok(out) = out
                    && out.status.success()
                {
                    let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                    if let Ok(mut s) = slot.lock() {
                        *s = Some(path.into());
                    }
                    ctx.request_repaint();
                }
            });
        }

        // Preview.
        self.pick_preview(ui, right, o);

        // Footer.
        let footer = Rect::from_min_max(Pos2::new(content.left(), content.bottom() - 46.0), content.max);
        let sel_name = if o.selected == CUSTOM_KEY {
            self.s.custom.as_ref().map(|c| c.name.clone())
        } else {
            Catalog::get().find(&o.selected).map(Entry::display_name)
        };
        ui.painter().text(
            footer.left_center(),
            Align2::LEFT_CENTER,
            sel_name.as_ref().map_or("Nothing selected yet".to_owned(), |n| format!("Selected: {n}")),
            body(14.0),
            MIST,
        );
        let (mut back, mut next) = (false, false);
        ui.scope_builder(UiBuilder::new().max_rect(footer).layout(Layout::right_to_left(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.add_enabled_ui(sel_name.is_some(), |ui| {
                next = primary_button(ui, if o.add_only { "Use these headphones" } else { "Continue" }).clicked();
            });
            back = secondary_button(ui, if o.add_only { "Cancel" } else { "Back" }).clicked();
        });
        if (next
            || confirm
            || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && sel_name.is_some()))
            && sel_name.is_some()
        {
            let key = o.selected.clone();
            self.select_headphones(&key);
            if o.add_only {
                self.notify(format!("Now calibrating {}", self.headphone_name()));
                return Act::Finish;
            }
            return Act::Go(Step::Sound);
        }
        if back || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            return if o.add_only { Act::Finish } else { Act::Go(Step::Access) };
        }
        Act::Stay
    }

    fn pick_preview(&mut self, ui: &mut Ui, rect: Rect, o: &mut Onboarding) {
        paint_glass(ui.painter(), rect, 28);
        let inner = rect.shrink2(Vec2::new(24.0, 22.0));
        let p = ui.painter();
        p.text(inner.left_top(), Align2::LEFT_TOP, "PREVIEW", medium(11.0), MIST);
        let custom = o.selected == CUSTOM_KEY;
        let entry = Catalog::get().find(&o.selected);
        let (name, source, rig, target_name) = match (entry, custom) {
            (Some(e), _) => (
                match e.variant {
                    Some(v) => format!("{} · {}", e.model, v.to_lowercase()),
                    None => format!("{} · stock", e.model),
                },
                e.source.to_owned(),
                e.rig.map(str::to_owned),
                Catalog::get().target_name(e).to_owned(),
            ),
            (None, true) => (
                self.s.custom.as_ref().map_or("Your measurement".into(), |c| c.name.clone()),
                "you".into(),
                None,
                profiles::TARGETS[0].name.into(),
            ),
            (None, false) => {
                p.text(
                    inner.center(),
                    Align2::CENTER_CENTER,
                    "Pick headphones to see their response",
                    body(15.0),
                    MIST,
                );
                return;
            }
        };
        let ng = p.layout(name, title(26.0), MILK, inner.width());
        let nh = ng.size().y;
        p.galley(inner.left_top() + Vec2::new(0.0, 22.0), ng, MILK);
        let ly = inner.top() + 30.0 + nh;
        p.line_segment(
            [Pos2::new(inner.left(), ly + 8.0), Pos2::new(inner.left() + 16.0, ly + 8.0)],
            Stroke::new(2.0, HALO),
        );
        let model = entry.map_or("Your measurement", |e| e.model);
        let r = p.text(
            Pos2::new(inner.left() + 22.0, ly),
            Align2::LEFT_TOP,
            format!("{model} as measured"),
            body(12.5),
            MIST,
        );
        for k in 0..4 {
            p.circle_filled(Pos2::new(r.right() + 20.0 + k as f32 * 4.5, ly + 8.0), 1.1, alpha(MILK, 0.7));
        }
        p.text(Pos2::new(r.right() + 42.0, ly), Align2::LEFT_TOP, format!("{target_name} target"), body(12.5), MIST);

        // Fit in the background (cached per entry).
        let (before, after, curves) = if custom {
            match &self.design {
                Some(d) if self.s.headphone_key == CUSTOM_KEY => (
                    Some(d.error_before_db),
                    Some(d.error_after_db),
                    Some((
                        d.grid.clone(),
                        d.measured.iter().map(|&v| v as f32).collect::<Vec<_>>(),
                        d.target.iter().map(|&v| v as f32).collect::<Vec<_>>(),
                    )),
                ),
                _ => (None, None, None),
            }
        } else {
            let cache = o.previews.lock().ok();
            match cache.as_ref().and_then(|c| c.get(&o.selected)) {
                Some(pv) => {
                    (Some(pv.before), Some(pv.after), Some((pv.grid.clone(), pv.measured.clone(), pv.target.clone())))
                }
                None => {
                    drop(cache);
                    if o.preview_pending.as_deref() != Some(o.selected.as_str()) {
                        o.preview_pending = Some(o.selected.clone());
                        let (key, slot, ctx) = (o.selected.clone(), o.previews.clone(), ui.ctx().clone());
                        std::thread::spawn(move || {
                            let cat = Catalog::get();
                            let Some(e) = cat.find(&key) else { return };
                            let params = DesignParams { bass_db: e.default_bass_db, ..Default::default() };
                            let d = fit::design(&cat.curve(e), &cat.target(e), &params, 48_000.0);
                            let pv = Preview {
                                measured: d.measured.iter().map(|&v| v as f32).collect(),
                                target: d.target.iter().map(|&v| v as f32).collect(),
                                grid: d.grid,
                                before: d.error_before_db,
                                after: d.error_after_db,
                            };
                            if let Ok(mut c) = slot.lock() {
                                c.insert(key, pv);
                            }
                            ctx.request_repaint();
                        });
                    }
                    (None, None, None)
                }
            }
        };
        let numbers_h = 150.0;
        let plot = Rect::from_min_max(
            Pos2::new(inner.left(), ly + 30.0),
            Pos2::new(inner.right(), inner.bottom() - numbers_h),
        );
        if let Some((grid, measured, target)) = &curves {
            ui.scope_builder(UiBuilder::new().max_rect(plot), |ui| {
                let c = graph::Curves { grid, measured, target, eq: &[], result: &[] };
                let layers = Layers { measured: true, target: true, eq: false, result: false, rta: false };
                graph::draw(ui, plot.size(), &c, None, &layers, 0.6);
            });
        } else {
            ui.painter().text(plot.center(), Align2::CENTER_CENTER, "fitting…", body(14.0), MIST);
        }
        let p = ui.painter();
        let ny = plot.bottom() + 16.0;
        let fmt = |v: Option<f64>| v.map_or("—".to_owned(), |v| format!("{v:.1}"));
        let b = p.layout_no_wrap(fmt(before), thin(50.0), MILK);
        let bw = b.size().x;
        p.galley(Pos2::new(inner.left(), ny), b, MILK);
        p.text(
            Pos2::new(inner.left() + bw + 8.0, ny + 14.0),
            Align2::LEFT_TOP,
            "dB off target\nas it is",
            body(12.0),
            MIST,
        );
        let ax = inner.left() + bw + 120.0;
        p.text(Pos2::new(ax - 22.0, ny + 30.0), Align2::CENTER_CENTER, "›", body(24.0), MIST);
        let a = p.layout_no_wrap(fmt(after), thin(50.0), SOLEIL);
        let aw = a.size().x;
        p.galley(Pos2::new(ax, ny), a, SOLEIL);
        p.text(Pos2::new(ax + aw + 8.0, ny + 14.0), Align2::LEFT_TOP, "dB with\nFlatline", body(12.0), MIST);
        let fy = inner.bottom() - 48.0;
        p.line_segment(
            [Pos2::new(inner.left(), fy - 10.0), Pos2::new(inner.right(), fy - 10.0)],
            Stroke::new(1.0, alpha(Color32::WHITE, 0.1)),
        );
        let measured_by = match rig {
            Some(r) => format!("Measured by {source} on {r}"),
            None if custom => "Your own measurement".to_owned(),
            None => format!("Measured by {source}"),
        };
        p.text(Pos2::new(inner.left(), fy), Align2::LEFT_TOP, measured_by, body(13.0), MIST);
        p.text(
            Pos2::new(inner.left(), fy + 22.0),
            Align2::LEFT_TOP,
            format!("Corrected towards {target_name}, with its bass shelf"),
            body(13.0),
            MIST,
        );
    }

    // ------------------------------------------------------- A4 sound --

    fn onb_sound(&mut self, ui: &mut Ui, full: Rect, o: &mut Onboarding) -> Act {
        self.onb_header(ui, full, o);
        let entry = self.entry();
        let default_bass = entry.map_or(6.0, |e| e.default_bass_db);
        let list = presets(default_bass);
        // Target shapes for the four presets (on a coarse grid).
        if o.sound_curves.as_ref().is_none_or(|(k, _, _)| k != &self.s.headphone_key) {
            let base = entry.map_or_else(|| profiles::TARGETS[0].curve(), |e| Catalog::get().target(e));
            let grid: Vec<f64> = (0..96).map(|i| 20.0 * 1000f64.powf(i as f64 / 95.0)).collect();
            let curves = list
                .iter()
                .map(|pr| {
                    let t = fit::build_target(&base, &grid, pr.bass, pr.tilt);
                    let off = crate::dsp::curve::band_mean(&grid, &t, 300.0, 3000.0);
                    t.iter().map(|v| (v - off) as f32).collect()
                })
                .collect();
            o.sound_curves = Some((self.s.headphone_key.clone(), grid, curves));
        }
        let mx = 32.0;
        let content = Rect::from_min_max(
            Pos2::new(full.left() + mx, full.top() + 76.0),
            Pos2::new(full.right() - mx, full.bottom() - 26.0),
        );
        let p = ui.painter();
        let tg = p.layout_job(widgets::tight("How do you like it?", display(48.0), MILK, -0.04));
        let th = tg.size().y;
        p.galley(content.min, tg, MILK);
        let (model, _) = self.headphone_label();
        p.text(
            Pos2::new(content.left(), content.top() + th + 4.0),
            Align2::LEFT_TOP,
            format!("Pick a target sound. Flatline bends your {model} towards it."),
            body(15.0),
            MIST,
        );
        let cards = Rect::from_min_max(
            Pos2::new(content.left(), content.top() + th + 44.0),
            Pos2::new(content.right(), content.bottom() - 70.0),
        );
        let gap = 16.0;
        let w = (cards.width() - 3.0 * gap) / 4.0;
        let curves = o.sound_curves.as_ref().map(|(_, _, c)| c.clone()).unwrap_or_default();
        for (i, pr) in list.iter().enumerate() {
            let r = Rect::from_min_size(
                Pos2::new(cards.left() + i as f32 * (w + gap), cards.top()),
                Vec2::new(w, cards.height()),
            );
            let resp = ui.interact(r, ui.scope_id().with(("preset", i)), Sense::click());
            if resp.clicked() {
                o.preset = i;
            }
            let sel = o.preset == i;
            let p = ui.painter();
            if sel {
                p.rect_filled(r, CornerRadius::same(26), MILK);
            } else {
                paint_glass(p, r, 26);
                if resp.hovered() {
                    p.rect_filled(r, CornerRadius::same(26), alpha(Color32::WHITE, 0.04));
                }
            }
            let ink = if sel { NIGHT } else { MILK };
            let sub = if sel { alpha(NIGHT, 0.6) } else { MIST };
            p.text(r.left_top() + Vec2::new(22.0, 20.0), Align2::LEFT_TOP, pr.name, title(28.0), ink);
            let rc = Pos2::new(r.right() - 32.0, r.top() + 38.0);
            if sel {
                p.circle_filled(rc, 12.0, NIGHT);
                widgets::draw_icon(p, Icon::Check, rc, 15.0, MILK);
            } else {
                p.circle_stroke(rc, 11.0, Stroke::new(1.5, alpha(MILK, 0.45)));
            }
            if i == 2 {
                badge(p, r.left_top() + Vec2::new(20.0, 62.0), "Recommended");
            } else {
                p.text(r.left_top() + Vec2::new(22.0, 66.0), Align2::LEFT_TOP, "Target curve", body(12.0), sub);
            }
            let chart = Rect::from_min_max(
                r.left_top() + Vec2::new(16.0, 96.0),
                Pos2::new(r.right() - 16.0, r.bottom() - 112.0),
            );
            p.rect_filled(
                chart,
                CornerRadius::same(14),
                if sel { alpha(NIGHT, 0.06) } else { alpha(Color32::WHITE, 0.05) },
            );
            if let Some(c) = curves.get(i) {
                let fy = |db: f32| chart.center().y + 10.0 - (db / 16.0).clamp(-1.0, 1.0) * chart.height() * 0.45;
                let pts: Vec<Pos2> = c
                    .iter()
                    .enumerate()
                    .map(|(k, &v)| Pos2::new(chart.left() + chart.width() * k as f32 / (c.len() - 1) as f32, fy(v)))
                    .collect();
                let (top, line) = if sel { (alpha(PULSE, 0.42), PULSE) } else { (alpha(PULSE, 0.30), MILK) };
                let mut m = Mesh::default();
                for (k, pt) in pts.iter().enumerate() {
                    m.colored_vertex(*pt, top);
                    m.colored_vertex(Pos2::new(pt.x, chart.bottom()), alpha(PULSE, 0.0));
                    if k > 0 {
                        let b = k as u32 * 2;
                        m.add_triangle(b - 2, b - 1, b);
                        m.add_triangle(b - 1, b + 1, b);
                    }
                }
                let painter = p.with_clip_rect(chart);
                painter.add(Shape::mesh(m));
                painter.line_segment(
                    [Pos2::new(chart.left(), fy(0.0)), Pos2::new(chart.right(), fy(0.0))],
                    Stroke::new(1.0, alpha(ink, 0.15)),
                );
                let x105 = chart.left() + chart.width() * ((105f32 / 20.0).ln() / 1000f32.ln());
                for s in Shape::dashed_line(
                    &[Pos2::new(x105, chart.top() + 8.0), Pos2::new(x105, chart.bottom() - 8.0)],
                    Stroke::new(1.0, alpha(ink, 0.18)),
                    3.0,
                    4.0,
                ) {
                    painter.add(s);
                }
                painter.line(pts, Stroke::new(2.2, line));
                p.text(
                    chart.left_top() + Vec2::new(12.0, 10.0),
                    Align2::LEFT_TOP,
                    format!("{:+.1} dB", pr.bass).replace("+0.0", "0"),
                    medium(12.5),
                    ink,
                );
            }
            p.text(Pos2::new(r.left() + 22.0, chart.bottom() + 22.0), Align2::LEFT_TOP, pr.blurb, body(16.0), ink);
            let shelf = format!("Bass shelf {:+.1} dB at 105 Hz", pr.bass).replace("+0.0", "0").replace(".0 dB", " dB");
            let shelf = if pr.tilt != 0.0 { format!("{shelf} · tilt {:+.1} dB/oct", pr.tilt) } else { shelf };
            p.text(Pos2::new(r.left() + 22.0, r.bottom() - 24.0), Align2::LEFT_BOTTOM, shelf, body(12.0), sub);
        }

        let footer = Rect::from_min_max(Pos2::new(content.left(), content.bottom() - 46.0), content.max);
        let p = ui.painter();
        widgets::draw_icon(p, Icon::Sliders, Pos2::new(footer.left() + 9.0, footer.center().y), 16.0, MIST);
        p.text(
            Pos2::new(footer.left() + 28.0, footer.center().y),
            Align2::LEFT_CENTER,
            "You can fine-tune Bass and Tilt any time.",
            body(14.0),
            MIST,
        );
        let (mut back, mut start) = (false, false);
        ui.scope_builder(UiBuilder::new().max_rect(footer).layout(Layout::right_to_left(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            start = primary_button(ui, "Start correcting").clicked();
            back = secondary_button(ui, "Back").clicked();
        });
        if start || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let pr = &list[o.preset.min(3)];
            self.s.params.bass_db = pr.bass;
            self.s.params.tilt_db_oct = pr.tilt;
            self.s.enabled = true;
            self.s.onboarded = true;
            // Menu-bar app: launch at login by default (Settings turns it off).
            if !self.demo && crate::login::bundle_path().is_some() && crate::login::set_enabled(true).is_ok() {
                self.login_enabled = crate::login::is_enabled();
            }
            if self.engine_device().is_none() && self.s.engine_on {
                self.start_engine();
            }
            self.notify("Flatline is on · press Space to compare");
            return Act::Finish;
        }
        if back {
            return Act::Go(Step::Pick);
        }
        Act::Stay
    }
}
