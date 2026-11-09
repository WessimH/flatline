//! The Horizon views: header, hero, chart card, control cards, the state
//! cards (bypass, paused, speakers, blocked, error), the
//! settings sheet, the headphone switcher and the overlays.

use super::super::aura::Mood;
use super::super::graph;
use super::super::theme::*;
use super::super::widgets::{self, Icon, Tone, lerp_color, paint_glass};
use super::onboarding::{Onboarding, presets};
use super::{BUFFER_SIZES, CUSTOM_KEY, FlatlineApp, SETTINGS_URL, Status};
use crate::catalog::Catalog;
use crate::profiles;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Layout, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind,
    Ui, UiBuilder, Vec2,
};
use std::time::Instant;

/// Splits a model name into one or two hero title lines
/// ("DT 770 Pro" → "DT 770", "Pro").
fn hero_title(model: &str) -> (String, Option<String>) {
    let words: Vec<&str> = model.split_whitespace().collect();
    if words.len() >= 2 && model.len() > 7 {
        let (a, b) = words.split_at(words.len() - 1);
        (a.join(" "), Some(b.join(" ")))
    } else {
        (model.trim().to_owned(), None)
    }
}

impl FlatlineApp {
    pub(super) fn mood(&self) -> Mood {
        match self.status {
            Status::Live | Status::Starting if self.s.enabled => Mood::Live,
            Status::Live | Status::Starting => Mood::Bypass,
            _ => Mood::Paused,
        }
    }

    fn paused(&self) -> bool {
        !matches!(self.status, Status::Live | Status::Starting)
    }

    /// The whole window.
    pub(super) fn view(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        let time = ui.input(|i| i.time);
        self.aura.paint(ui.painter(), full, time);

        let w = full.width();
        let h = full.height();
        let mx = 32.0;
        let header = Rect::from_min_size(full.min + Vec2::new(mx, 14.0), Vec2::new(w - 2.0 * mx, 52.0));
        let cards_h = if h > 860.0 { 270.0 } else { 232.0 };
        let cards = Rect::from_min_size(
            Pos2::new(full.left() + mx, full.bottom() - 26.0 - cards_h),
            Vec2::new(w - 2.0 * mx, cards_h),
        );
        let hero = Rect::from_min_max(
            Pos2::new(full.left() + mx, header.bottom() + 22.0),
            Pos2::new(full.right() - mx, cards.top() - 22.0),
        );
        let left_w = (w * 0.30).clamp(360.0, 440.0);
        let hero_left = Rect::from_min_max(hero.min, Pos2::new(hero.left() + left_w, hero.bottom()));
        let chart = Rect::from_min_max(Pos2::new(hero_left.right() + 26.0, hero.top()), hero.max);

        ui.scope_builder(UiBuilder::new().max_rect(header), |ui| self.header(ui, time));
        ui.scope_builder(UiBuilder::new().max_rect(hero_left).layout(Layout::top_down(Align::Min)), |ui| {
            self.hero(ui, time)
        });
        self.chart_card(ui, chart, time);
        ui.scope_builder(UiBuilder::new().max_rect(cards), |ui| {
            if self.paused() {
                ui.multiply_opacity(0.45);
            }
            self.cards(ui, cards, time)
        });

        if self.status == Status::Blocked {
            self.blocked_overlay(ui, full);
        }
        if self.switcher_open {
            ui.painter().rect_filled(full, 0.0, alpha(NIGHT, 0.6));
            self.switcher(ui, hero_left);
        } else {
            self.switcher_shown = false;
        }
        if self.settings_open {
            self.settings_sheet(ui, full);
        }
        self.overlays(ui.ctx());
    }

    // ---------------------------------------------------------- header --

    fn header(&mut self, ui: &mut Ui, time: f64) {
        let rect = ui.max_rect();
        // Drag the window by the header background (the title bar is hidden).
        let drag = ui.interact(rect, ui.scope_id().with("drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        // Logo after the traffic lights.
        ui.scope_builder(
            UiBuilder::new().max_rect(Rect::from_min_size(rect.min + Vec2::new(52.0, 10.0), Vec2::new(260.0, 34.0))),
            |ui| {
                widgets::logo(ui, 25.0);
            },
        );
        let device = self.engine_device().map(|d| d.name.clone()).unwrap_or_default();
        let rate = self.fs() / 1000.0;
        let (text, tone) = match self.status {
            Status::Live if self.s.enabled => (format!("Live · {device} · {rate:.0} kHz"), Tone::Live),
            Status::Live => (format!("Bypass · {device} · {rate:.0} kHz"), Tone::Neutral),
            Status::Starting => ("Starting…".to_owned(), Tone::Neutral),
            Status::Blocked => ("Blocked by macOS".to_owned(), Tone::Warning),
            Status::Error => ("Error".to_owned(), Tone::Error),
            Status::Standby => ("Off · system audio untouched".to_owned(), Tone::Neutral),
            Status::Asleep => ("Paused · Mac asleep".to_owned(), Tone::Neutral),
            Status::Speakers => (format!("Speakers · {}", self.speakers_name()), Tone::Neutral),
            Status::NoOutput => ("No output".to_owned(), Tone::Neutral),
        };
        if self.invite.is_some() && self.status != Status::Speakers {
            let banner = Rect::from_min_max(
                Pos2::new(rect.left() + 300.0, rect.center().y - 30.0),
                Pos2::new(rect.right() - 112.0, rect.center().y + 30.0),
            );
            self.new_output_banner(ui, banner, tone, time);
        } else {
            let pill_w = ui.painter().layout_no_wrap(text.clone(), body(14.0), MILK).size().x + 50.0;
            let pill = Rect::from_center_size(Pos2::new(rect.center().x, rect.center().y), Vec2::new(pill_w, 40.0));
            ui.scope_builder(UiBuilder::new().max_rect(pill), |ui| {
                widgets::status_pill(ui, &text, tone, time);
            });
        }
        let buttons = Rect::from_min_max(Pos2::new(rect.right() - 100.0, rect.top() + 4.0), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(buttons).layout(Layout::right_to_left(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if widgets::icon_button(ui, Icon::Sliders, 44.0).on_hover_text("Settings").clicked() {
                self.settings_open = !self.settings_open;
                self.switcher_open = false;
            }
            if widgets::icon_button(ui, Icon::Headphones, 44.0).on_hover_text("Your headphones").clicked() {
                self.switcher_open = !self.switcher_open;
                self.settings_open = false;
            }
        });
    }

    /// The banner on a new output: keep the current headphones on it, or
    /// choose others.
    fn new_output_banner(&mut self, ui: &mut Ui, rect: Rect, tone: Tone, time: f64) {
        paint_glass(ui.painter(), rect, 30);
        let output = self.invite.as_deref().map(|u| self.device_name(u)).unwrap_or_default();
        let model = short_model(&self.headphone_name());
        let state = match self.status {
            Status::Live if self.s.enabled => "Live",
            Status::Live => "Bypass",
            _ => "Starting",
        };
        let (mut yes, mut choose, mut close) = (false, false, false);
        let inner = rect.shrink2(Vec2::new(12.0, 0.0));
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            widgets::status_pill(ui, state, tone, time);
            let line1 = format!("New output: {output}");
            let line2 = format!("Still {model}?");
            let text_w = ui.painter().layout_no_wrap(line1.clone(), medium(16.0), MILK).size().x;
            ui.allocate_ui_with_layout(Vec2::new(text_w, 42.0), Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(line1).font(medium(16.0)).color(MILK));
                ui.label(RichText::new(line2).font(body(14.0)).color(MIST));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                close = widgets::glass_icon_button(ui, Icon::Close, 34.0).on_hover_text("Not now").clicked();
                choose = primary_button(ui, "Choose headphones").clicked();
                yes = secondary_button(ui, &format!("Yes, {model}")).clicked();
            });
        });
        if yes {
            let key = self.s.headphone_key.clone();
            self.select_headphones(&key);
        }
        if choose {
            self.switcher_open = true;
            self.settings_open = false;
        }
        if close {
            self.invite = None;
        }
    }

    // ------------------------------------------------------------ hero --

    fn hero(&mut self, ui: &mut Ui, time: f64) {
        let speakers = self.status == Status::Speakers;
        let (model, variant) = if speakers { ("Built-in Speakers".to_owned(), None) } else { self.headphone_label() };
        let (l1, l2) = hero_title(&model);
        ui.spacing_mut().item_spacing.y = 14.0;
        let eyebrow = match self.entry() {
            _ if speakers => "NOW PLAYING ON".to_owned(),
            Some(e) => format!("NOW CALIBRATING · {}", e.brand.to_uppercase()),
            None => "NOW CALIBRATING".to_owned(),
        };
        ui.label(widgets::eyebrow(&eyebrow, MIST));
        // Fit the title to the column.
        let width = ui.available_width();
        let mut size: f32 = 80.0;
        for line in [Some(&l1), l2.as_ref()].into_iter().flatten() {
            let g = ui.painter().layout_job(widgets::tight(line, display(size), MILK, -0.045));
            if g.size().x > width - 110.0 {
                size *= (width - 110.0) / g.size().x;
            }
        }
        let size = size.clamp(36.0, 80.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        if l2.is_none() {
            self.title_line(ui, &l1, size, true);
        } else {
            self.title_line(ui, &l1, size, false);
            self.title_line(ui, l2.as_deref().unwrap_or_default(), size, true);
        }
        ui.add_space(14.0);
        ui.spacing_mut().item_spacing.y = 14.0;
        let mut sub = format!("Tuned to {} · {} bands", self.target_name(), self.s.params.n_filters);
        if let Some(v) = variant {
            sub = format!("{} · {sub}", capitalize(&v));
        }
        if speakers {
            sub = format!("{} · no correction", self.speakers_name());
        }
        ui.label(RichText::new(sub).font(light(16.0)).color(MIST));
        let default_bass = self.entry().map_or(6.0, |e| e.default_bass_db);
        if !speakers {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                for pr in presets(default_bass) {
                    let sel = (self.s.params.bass_db - pr.bass).abs() < 0.05
                        && (self.s.params.tilt_db_oct - pr.tilt).abs() < 0.005;
                    if widgets::pill(ui, pr.name, sel).on_hover_text(pr.blurb).clicked() {
                        self.s.params.bass_db = pr.bass;
                        self.s.params.tilt_db_oct = pr.tilt;
                    }
                }
            });
        }

        // Power row pinned to the bottom of the column.
        let rest = ui.available_rect_before_wrap();
        let row = Rect::from_min_max(Pos2::new(rest.left(), rest.bottom() - 130.0), rest.max);
        ui.scope_builder(UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 22.0;
            let can_toggle = matches!(self.status, Status::Live | Status::Starting);
            let on = self.s.enabled && can_toggle;
            let r = widgets::power_button(ui, on, 108.0, time).on_hover_text("Correction on / bypass  (Space)");
            if r.clicked() && can_toggle {
                self.s.enabled = !self.s.enabled;
            }
            let hp_name = self.headphone_name();
            let (t, sub) = match self.status {
                Status::Live | Status::Starting if self.s.enabled => {
                    ("Correction on", "Press Space to hear the difference".to_owned())
                }
                Status::Live | Status::Starting => {
                    ("Bypass", format!("You are hearing the raw {}", short_model(&hp_name)))
                }
                Status::Speakers => ("No correction", "Built-in speakers".to_owned()),
                Status::NoOutput => ("Waiting", "No audio output on this Mac".to_owned()),
                Status::Asleep => ("Paused", "Your Mac is asleep".to_owned()),
                Status::Blocked => ("Blocked", "macOS is not sharing system audio".to_owned()),
                Status::Error => ("Stopped", "The audio engine could not start".to_owned()),
                Status::Standby => ("Off", "System audio is untouched".to_owned()),
            };
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(RichText::new(t).font(title(30.0)).color(MILK));
                ui.label(RichText::new(sub).font(body(14.5)).color(MIST));
            });
        });
    }

    fn title_line(&mut self, ui: &mut Ui, text: &str, size: f32, with_switch: bool) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let g = ui.painter().layout_job(widgets::tight(text, display(size), MILK, -0.045));
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(g.size().x, size * 0.92), Sense::click());
            ui.painter().galley(Pos2::new(rect.left(), rect.top() - size * 0.14), g, MILK);
            let mut open = resp.clicked();
            if with_switch {
                ui.add_space(4.0);
                let icon = if self.switcher_open { Icon::ChevronUp } else { Icon::ChevronDown };
                open |= widgets::glass_icon_button(ui, icon, 36.0).clicked();
                open |= widgets::tag(ui, "Change").clicked();
            }
            if open {
                self.switcher_open = !self.switcher_open;
                self.settings_open = false;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        });
    }

    // ----------------------------------------------------------- chart --

    fn chart_card(&mut self, ui: &mut Ui, rect: Rect, _time: f64) {
        self.chart_rect = rect;
        paint_glass(ui.painter(), rect, 30);
        let inner = rect.shrink2(Vec2::new(26.0, 22.0));
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
            let head = Rect::from_min_size(inner.min, Vec2::new(inner.width(), 70.0));
            let p = ui.painter();
            p.text(head.left_top(), Align2::LEFT_TOP, "Frequency response", medium(17.0), MILK);
            let name = if self.status == Status::Speakers { "Speakers".to_owned() } else { self.headphone_name() };
            let legend = [("Result".to_owned(), SOLEIL), (short_model(&name), HALO), ("Target".to_owned(), MILK)];
            let mut x = head.left();
            for (k, (label, col)) in legend.into_iter().enumerate() {
                let y = head.top() + 34.0;
                if k == 2 {
                    for k in 0..4 {
                        p.circle_filled(Pos2::new(x + 2.0 + k as f32 * 5.0, y + 8.0), 1.2, alpha(col, 0.7));
                    }
                } else if k == 0 {
                    for k in 0..18 {
                        let t = k as f32 / 18.0;
                        let c = if t < 0.5 {
                            lerp_color(SOLEIL, MILK, t * 2.0)
                        } else {
                            lerp_color(MILK, HALO, (t - 0.5) * 2.0)
                        };
                        p.line_segment(
                            [Pos2::new(x + k as f32, y + 8.0), Pos2::new(x + k as f32 + 1.0, y + 8.0)],
                            Stroke::new(3.0, c),
                        );
                    }
                } else {
                    p.line_segment([Pos2::new(x, y + 8.0), Pos2::new(x + 18.0, y + 8.0)], Stroke::new(2.0, col));
                }
                let r = p.text(Pos2::new(x + 26.0, y), Align2::LEFT_TOP, label, body(13.5), MIST);
                x = r.right() + 20.0;
            }
            // Deviation from target, huge and thin.
            let (before, after) = self.design.as_ref().map_or((0.0, 0.0), |d| (d.error_before_db, d.error_after_db));
            let value = match self.status {
                Status::Live | Status::Starting if self.s.enabled => format!("{after:.1}"),
                Status::Live | Status::Starting => format!("{before:.1}"),
                _ => "—".to_owned(),
            };
            let unit = p.layout_no_wrap("dB\noff target".into(), body(14.0), MIST);
            let num = p.layout_no_wrap(value, thin(66.0), MILK);
            let ux = head.right() - unit.size().x;
            p.galley(Pos2::new(ux, head.top() + 22.0), unit, MIST);
            p.galley(Pos2::new(ux - 10.0 - num.size().x, head.top() - 6.0), num, MILK);

            let plot = Rect::from_min_max(Pos2::new(inner.left(), head.bottom() + 12.0), inner.max);
            ui.scope_builder(UiBuilder::new().max_rect(plot), |ui| {
                let curves = graph::Curves {
                    grid: self.design.as_ref().map_or(&[][..], |d| &d.grid[..]),
                    measured: &self.disp.measured,
                    target: &self.disp.target,
                    eq: &self.disp.eq,
                    result: &self.disp.result,
                };
                let rta =
                    graph::Rta { freqs: &self.analyzer.freqs, pre: &self.analyzer.db[0], post: &self.analyzer.db[1] };
                let bypass =
                    ui.ctx().animate_bool_with_time(ui.scope_id().with("bypass"), self.mood() != Mood::Live, 0.42);
                if self.paused() {
                    ui.multiply_opacity(0.3);
                }
                let live = self.status == Status::Live;
                graph::draw(ui, plot.size(), &curves, live.then_some(&rta), &self.s.layers, bypass);
            });
        });

        // State cards over the chart.
        match self.status {
            Status::Speakers => self.speakers_card(ui, rect),
            Status::NoOutput => self.no_output_card(ui, rect),
            Status::Error => self.error_card(ui, rect),
            Status::Asleep => self.asleep_card(ui, rect),
            _ => {}
        }
    }

    /// A state card of `height`, centred in the plot of the chart at `area`.
    fn card_rect(area: Rect, height: f32) -> Rect {
        // Below the chart header and legend.
        let plot = Rect::from_min_max(area.min + Vec2::new(0.0, 92.0), area.max);
        Rect::from_center_size(plot.center(), Vec2::new((area.width() - 120.0).min(560.0), height))
    }

    fn state_card(
        ui: &mut Ui,
        card: Rect,
        icon: Icon,
        color: Color32,
        title_text: &str,
        body_text: &str,
        add: impl FnOnce(&mut Ui),
    ) {
        let p = ui.painter();
        p.rect_filled(card, CornerRadius::same(28), Color32::from_rgba_unmultiplied(26, 22, 28, 238));
        p.rect_stroke(card, CornerRadius::same(28), Stroke::new(1.0, alpha(color, 0.35)), StrokeKind::Inside);
        let ic = Pos2::new(card.center().x, card.top() + 56.0);
        p.circle_filled(ic, 28.0, alpha(color, 0.14));
        p.circle_stroke(ic, 28.0, Stroke::new(1.0, alpha(color, 0.5)));
        widgets::draw_icon(p, icon, ic, 26.0, color);
        let inner = Rect::from_min_max(
            Pos2::new(card.left() + 36.0, card.top() + 100.0),
            Pos2::new(card.right() - 36.0, card.bottom() - 24.0),
        );
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.label(RichText::new(title_text).font(title(26.0)).color(MILK));
            ui.label(RichText::new(body_text).font(body(15.0)).color(MIST));
            ui.add_space(8.0);
            add(ui);
        });
    }

    fn speakers_card(&mut self, ui: &mut Ui, area: Rect) {
        Self::state_card(
            ui,
            Self::card_rect(area, 290.0),
            Icon::Speaker,
            MIST,
            "Speakers aren’t corrected",
            "Flatline corrects headphones. Connect a pair and its profile loads on its own.",
            |ui| Self::listening(ui, "Listening for headphones…"),
        );
    }

    fn no_output_card(&mut self, ui: &mut Ui, area: Rect) {
        Self::state_card(
            ui,
            Self::card_rect(area, 290.0),
            Icon::Headphones,
            MIST,
            "No audio output",
            "Your Mac has no output to play on. Connect headphones and Flatline starts on its own.",
            |ui| Self::listening(ui, "Listening for an output…"),
        );
    }

    /// A centred "listening" line with a dot, for the waiting cards.
    fn listening(ui: &mut Ui, label: &str) {
        ui.horizontal(|ui| {
            let text = ui.painter().layout_no_wrap(label.into(), body(14.5), MILK);
            ui.add_space((ui.available_width() - text.size().x - 14.0) / 2.0);
            let (r, _) = ui.allocate_exact_size(Vec2::new(14.0 + text.size().x, 20.0), Sense::hover());
            let p = ui.painter();
            p.circle_filled(Pos2::new(r.left() + 4.0, r.center().y), 3.5, MILK);
            p.galley(Pos2::new(r.left() + 14.0, r.center().y - text.size().y / 2.0), text, MILK);
        });
    }

    fn error_card(&mut self, ui: &mut Ui, area: Rect) {
        let detail = self.engine_error.clone().unwrap_or_default();
        let device = self
            .s
            .output_uid
            .as_ref()
            .or(self.default_uid.as_ref())
            .and_then(|u| self.devices.iter().find(|d| &d.uid == u))
            .map_or("this output".to_owned(), |d| d.name.clone());
        let (mut retry, mut other) = (false, false);
        Self::state_card(
            ui,
            Self::card_rect(area, 344.0),
            Icon::Warning,
            ERROR,
            &format!("Can't start audio on {device}"),
            "Another app may be using it exclusively, or it doesn't support this sample rate.",
            |ui| {
                ui.label(RichText::new(detail.chars().take(90).collect::<String>()).font(mono(12.0)).color(MIST2));
                ui.horizontal(|ui| {
                    retry = primary_button(ui, "Retry").clicked();
                    other = secondary_button(ui, "Choose another output").clicked();
                });
            },
        );
        if retry {
            self.start_engine();
        }
        if other {
            self.settings_open = true;
        }
    }

    fn asleep_card(&mut self, ui: &mut Ui, area: Rect) {
        Self::state_card(
            ui,
            Self::card_rect(area, 344.0),
            Icon::Moon,
            MIST,
            "Your Mac is asleep",
            "Flatline resumes when it wakes up.",
            |_| {},
        );
    }

    pub(super) fn blocked_overlay(&mut self, ui: &mut Ui, full: Rect) {
        let p = ui.painter();
        p.rect_filled(full, 0.0, alpha(NIGHT, 0.72));
        let card = Rect::from_center_size(full.center(), Vec2::new(760.0, 430.0));
        // Swallow clicks meant for the controls underneath.
        ui.interact(full, ui.scope_id().with("blocked-veil"), Sense::click());
        p.rect_filled(card, CornerRadius::same(30), Color32::from_rgba_unmultiplied(26, 22, 28, 245));
        p.rect_stroke(card, CornerRadius::same(30), Stroke::new(1.0, glass_edge()), StrokeKind::Inside);
        let ic = card.left_top() + Vec2::new(66.0, 66.0);
        p.circle_filled(ic, 28.0, alpha(AMBER, 0.14));
        p.circle_stroke(ic, 28.0, Stroke::new(1.0, alpha(AMBER, 0.5)));
        widgets::draw_icon(p, Icon::Lock, ic, 26.0, AMBER);
        let inner = Rect::from_min_max(card.min + Vec2::new(116.0, 40.0), card.max - Vec2::new(40.0, 30.0));
        let (mut open, mut retry, mut copy) = (false, false, false);
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
            ui.spacing_mut().item_spacing.y = 12.0;
            ui.label(RichText::new("macOS is blocking system audio").font(title(28.0)).color(MILK));
            ui.label(
                RichText::new("Allow Flatline in System Settings, then come back: it retries on its own. Your audio plays normally, uncorrected, meanwhile.")
                    .font(body(15.0))
                    .color(MIST),
            );
            ui.add_space(6.0);
            for (n, step) in ["Open System Settings › Privacy & Security", "Choose “Screen & System Audio Recording”, then “System Audio Recording Only”", "Turn Flatline on"].iter().enumerate() {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                    ui.painter().circle_stroke(r.center(), 13.0, Stroke::new(1.0, alpha(Color32::WHITE, 0.3)));
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, (n + 1).to_string(), medium(13.0), MILK);
                    ui.label(RichText::new(*step).font(body(14.5)).color(MILK));
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                open = primary_button(ui, "Open System Settings").clicked();
                retry = secondary_button(ui, "Retry now").clicked();
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Still blocked after allowing? Reset the permission in Terminal, then allow again:").font(body(13.5)).color(MIST2));
            ui.horizontal(|ui| {
                ui.label(RichText::new("tccutil reset All dev.flatline.app").font(mono(13.0)).color(MILK));
                copy = widgets::glass_icon_button(ui, Icon::Copy, 30.0).on_hover_text("Copy").clicked();
            });
        });
        if open {
            let _ = std::process::Command::new("open").arg(SETTINGS_URL).spawn();
        }
        if retry {
            self.blocked_retries = 0;
            self.start_engine();
        }
        if copy {
            ui.ctx().copy_text("tccutil reset All dev.flatline.app".into());
            self.notify("Command copied");
        }
    }

    // ----------------------------------------------------------- cards --

    fn cards(&mut self, ui: &mut Ui, rect: Rect, time: f64) {
        let gap = 18.0;
        let w = (rect.width() - 3.0 * gap) / 4.0;
        let card = |i: usize| {
            Rect::from_min_size(Pos2::new(rect.left() + i as f32 * (w + gap), rect.top()), Vec2::new(w, rect.height()))
        };
        for i in 0..4 {
            paint_glass(ui.painter(), card(i), 28);
        }
        let head = |ui: &mut Ui, r: Rect, t: &str, sub: &str| {
            let p = ui.painter();
            p.text(r.left_top() + Vec2::new(22.0, 20.0), Align2::LEFT_TOP, t, medium(16.5), MILK);
            p.text(r.right_top() + Vec2::new(-22.0, 22.0), Align2::RIGHT_TOP, sub, body(13.5), MIST);
        };

        // Amount.
        let r = card(0);
        head(ui, r, "Amount", "how much to correct");
        let body_r = Rect::from_min_max(r.min + Vec2::new(20.0, 56.0), r.max - Vec2::new(20.0, 20.0));
        ui.scope_builder(UiBuilder::new().max_rect(body_r), |ui| {
            widgets::amount_slider(ui, body_r.size(), &mut self.s.amount, 1.0);
        });

        // Bass.
        let r = card(1);
        head(ui, r, "Bass", "shelf at 105 Hz");
        let body_r = Rect::from_min_max(r.min + Vec2::new(16.0, 52.0), r.max - Vec2::new(16.0, 12.0));
        ui.scope_builder(UiBuilder::new().max_rect(body_r), |ui| {
            widgets::dial(ui, body_r.size(), &mut self.s.params.bass_db, -6.0..=12.0, 6.5);
        });

        // Latency.
        let r = card(2);
        head(ui, r, "Latency", "end to end");
        let info = self.engine.as_ref().map(|e| e.info.clone());
        let offset = self.shared.io_offset.load() as f64;
        let ms = info.as_ref().map(|i| {
            if self.status == Status::Live && offset > 0.0 {
                offset / i.sample_rate * 1000.0
            } else {
                2.0 * i.buffer_frames as f64 / i.sample_rate * 1000.0
            }
        });
        let p = ui.painter();
        let size = (r.height() * 0.4).min(104.0);
        let num = p.layout_no_wrap(ms.map_or("—".into(), |m| format!("{m:.1}")), thin(size), MILK);
        let unit = p.layout_no_wrap("ms".into(), body(size * 0.27), MILK);
        let base = r.left_top() + Vec2::new(18.0, 48.0);
        let num_size = num.size();
        p.galley(base, num, MILK);
        p.galley(Pos2::new(base.x + num_size.x + 4.0, base.y + num_size.y * 0.80 - unit.size().y), unit, MILK);
        p.text(
            r.left_bottom() + Vec2::new(22.0, -20.0),
            Align2::LEFT_BOTTOM,
            "The EQ itself adds 0 samples.",
            body(14.0),
            MIST,
        );
        let hb = Rect::from_min_max(
            Pos2::new(r.left() + 22.0, r.bottom() - 74.0),
            Pos2::new(r.right() - 22.0, r.bottom() - 46.0),
        );
        let live = self.status == Status::Live;
        widgets::heartbeat(p, hb, if live { time } else { 0.0 }, SOLEIL);

        // Output.
        let r = card(3);
        let clip = self.clip_until.is_some_and(|t| Instant::now() < t);
        let sub = if !live {
            "no signal"
        } else if clip {
            "clipping"
        } else {
            "live"
        };
        head(ui, r, "Output", sub);
        let bars_r = Rect::from_min_max(r.min + Vec2::new(22.0, 64.0), r.max - Vec2::new(22.0, 50.0));
        let active = if self.mood() == Mood::Live { 1.0 } else { 0.0 };
        widgets::bars(ui.painter(), bars_r, &self.bar_levels, active);
        let peak = self.meters.level[2].max(self.meters.level[3]);
        let peak_txt = if live && peak > -69.0 { format!("Peak {peak:.0} dB") } else { "Peak —".to_owned() };
        let p = ui.painter();
        p.text(
            Pos2::new(r.left() + 22.0, r.bottom() - 22.0),
            Align2::LEFT_BOTTOM,
            peak_txt,
            body(13.5),
            if clip { ERROR } else { MIST },
        );
        p.text(
            Pos2::new(r.right() - 22.0, r.bottom() - 22.0),
            Align2::RIGHT_BOTTOM,
            format!("Preamp {:+.1} dB", self.preamp_db),
            body(13.5),
            MIST,
        );
    }

    // ------------------------------------------------- switcher & sheet --

    /// The headphone profiles: headphones and the devices they are linked to.
    /// Each row: (key, name, detail, linked to a device, device connected).
    fn profiles(&self) -> Vec<(String, String, String, bool, bool)> {
        let cat = Catalog::get();
        let mut keys: Vec<String> = self.s.my_headphones.iter().filter(|k| cat.find(k).is_some()).cloned().collect();
        for o in &self.s.headphone_outputs {
            if !keys.contains(&o.headphones)
                && (cat.find(&o.headphones).is_some() || o.headphones == CUSTOM_KEY && self.s.custom.is_some())
            {
                keys.push(o.headphones.clone());
            }
        }
        if self.s.custom.is_some() && !keys.iter().any(|k| k == CUSTOM_KEY) {
            keys.push(CUSTOM_KEY.to_owned());
        }
        keys.into_iter()
            .map(|key| {
                let links: Vec<&super::HeadphoneOutput> =
                    self.s.headphone_outputs.iter().filter(|o| o.headphones == key).collect();
                let connected = links.iter().any(|o| self.devices.iter().any(|d| d.uid == o.uid));
                let detail = if !links.is_empty() {
                    let names: Vec<&str> = links.iter().map(|o| o.name.as_str()).collect();
                    format!("on {}", names.join(", "))
                } else if let Some(e) = cat.find(&key) {
                    format!("{} · measured by {}", e.brand, e.source)
                } else {
                    "Your measurement (CSV)".to_owned()
                };
                (key.clone(), self.headphones_name(&key), detail, !links.is_empty(), connected)
            })
            .collect()
    }

    fn switcher(&mut self, ui: &mut Ui, hero_left: Rect) {
        let ctx = ui.ctx().clone();
        let pos = hero_left.left_top() + Vec2::new(0.0, 210.0);
        let mut pick: Option<String> = None;
        let (mut close, mut add_new) = (false, false);
        let profiles = self.profiles();
        let resp = egui::Area::new(egui::Id::unique("switcher")).order(egui::Order::Foreground).fixed_pos(pos).show(
            &ctx,
            |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(28, 24, 30))
                    .stroke(Stroke::new(1.0, glass_edge()))
                    .corner_radius(CornerRadius::same(24))
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_width(436.0);
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.label(widgets::eyebrow("MY HEADPHONES", MIST));
                        });
                        ui.add_space(6.0);
                        for (key, name, detail, linked, connected) in profiles {
                            let row = headphone_row(
                                ui,
                                &name,
                                &detail,
                                linked.then_some(connected),
                                key == self.s.headphone_key,
                            );
                            if row.clicked() {
                                pick = Some(key);
                            }
                        }
                        ui.add_space(2.0);
                        let (sep, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                        ui.painter()
                            .line_segment([sep.left_center(), sep.right_center()], Stroke::new(1.0, glass_edge()));
                        let (r, add) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 62.0), Sense::click());
                        let p = ui.painter();
                        if add.hovered() {
                            p.rect_filled(r, CornerRadius::same(14), alpha(Color32::WHITE, 0.06));
                        }
                        let c = Pos2::new(r.left() + 30.0, r.center().y);
                        let ring: Vec<Pos2> = (0..=48)
                            .map(|i| {
                                let a = i as f32 / 48.0 * std::f32::consts::TAU;
                                c + Vec2::new(a.cos(), a.sin()) * 19.0
                            })
                            .collect();
                        for s in
                            egui::Shape::dashed_line(&ring, Stroke::new(1.0, alpha(Color32::WHITE, 0.35)), 3.0, 3.0)
                        {
                            p.add(s);
                        }
                        widgets::draw_icon(p, Icon::Plus, c, 18.0, MILK);
                        p.text(
                            Pos2::new(r.left() + 64.0, r.center().y),
                            Align2::LEFT_CENTER,
                            "Add headphones…",
                            medium(15.0),
                            MILK,
                        );
                        if add.clicked() {
                            add_new = true;
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(
                                    "Flatline loads a profile when you switch output. Speakers are never corrected.",
                                )
                                .font(body(13.0))
                                .color(MIST2),
                            );
                        });
                        ui.add_space(4.0);
                    });
            },
        );
        if let Some(k) = pick {
            // Links the profile to the output playing now (never the speakers).
            self.select_headphones(&k);
            close = true;
        }
        if add_new {
            self.onb = Some(Onboarding::picker());
            close = true;
        }
        // Close on a click elsewhere (not on the frame that opened it).
        let fresh = !self.switcher_shown;
        self.switcher_shown = true;
        let outside = ctx.input(|i| {
            i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !resp.response.rect.contains(p))
        });
        if close || (outside && !fresh) {
            self.switcher_open = false;
        }
    }

    /// Settings: one row per device and the headphones linked to it, with
    /// Forget.
    fn profiles_section(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(widgets::eyebrow("HEADPHONE PROFILES", MIST2));
        ui.label(
            RichText::new("Each profile is linked to one audio device. Flatline loads it when you use that device.")
                .font(body(13.5))
                .color(MIST),
        );
        ui.add_space(4.0);
        let playing = self.want_uid();
        let rows: Vec<(String, String, bool, bool)> = self
            .s
            .headphone_outputs
            .iter()
            .map(|o| {
                let connected = self.devices.iter().any(|d| d.uid == o.uid);
                let in_use = playing.as_deref() == Some(o.uid.as_str()) && o.headphones == self.s.headphone_key;
                (self.headphones_name(&o.headphones), format!("Linked to {}", o.name), connected, in_use)
            })
            .collect();
        let mut forget = None;
        egui::Frame::new()
            .fill(alpha(Color32::WHITE, 0.04))
            .stroke(Stroke::new(1.0, glass_edge()))
            .corner_radius(CornerRadius::same(18))
            .inner_margin(egui::Margin::symmetric(14, 6))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if rows.is_empty() {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(
                            "No profile yet. Pick your headphones: Flatline links them to the device playing.",
                        )
                        .font(body(13.5))
                        .color(MIST2),
                    );
                    ui.add_space(10.0);
                }
                for (i, (name, detail, connected, in_use)) in rows.into_iter().enumerate() {
                    if i > 0 {
                        let (sep, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                        ui.painter()
                            .line_segment([sep.left_center(), sep.right_center()], Stroke::new(1.0, glass_edge()));
                    }
                    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 68.0), Sense::hover());
                    // Right side: Forget, then the "In use" badge.
                    let mut right = r.right();
                    ui.scope_builder(
                        UiBuilder::new()
                            .max_rect(Rect::from_min_max(Pos2::new(r.right() - 90.0, r.top()), r.max))
                            .layout(Layout::right_to_left(Align::Center)),
                        |ui| {
                            let f = widgets::tag(ui, "Forget").on_hover_text("Unlink these headphones from the device");
                            right = f.rect.left();
                            if f.clicked() {
                                forget = Some(i);
                            }
                        },
                    );
                    let p = ui.painter();
                    if in_use {
                        let g = p.layout_no_wrap("In use".into(), medium(12.0), SOLEIL);
                        let b = Rect::from_min_size(
                            Pos2::new(right - 10.0 - g.size().x - 30.0, r.center().y - 13.0),
                            Vec2::new(g.size().x + 30.0, 26.0),
                        );
                        p.rect_filled(b, CornerRadius::same(13), alpha(SOLEIL, 0.12));
                        p.rect_stroke(
                            b,
                            CornerRadius::same(13),
                            Stroke::new(1.0, alpha(SOLEIL, 0.45)),
                            StrokeKind::Inside,
                        );
                        p.circle_filled(Pos2::new(b.left() + 12.0, b.center().y), 3.0, SOLEIL);
                        p.galley(Pos2::new(b.left() + 20.0, b.center().y - g.size().y / 2.0), g, SOLEIL);
                        right = b.left();
                    }
                    let (ink, sub) = if connected { (MILK, MIST) } else { (MIST2, alpha(MIST2, 0.8)) };
                    let c = Pos2::new(r.left() + 20.0, r.center().y);
                    p.circle_stroke(c, 19.0, Stroke::new(1.0, alpha(Color32::WHITE, 0.25)));
                    widgets::draw_icon(p, Icon::Headphones, c, 18.0, ink);
                    let x = r.left() + 54.0;
                    let width = right - 10.0 - x;
                    p.galley(Pos2::new(x, r.top() + 14.0), clipped(p, &name, medium(15.0), ink, width), ink);
                    let state_w = if connected { 82.0 } else { 100.0 };
                    let d = clipped(p, &detail, body(12.5), sub, width - state_w);
                    let d_rect = Rect::from_min_size(Pos2::new(x, r.top() + 37.0), d.size());
                    p.galley(d_rect.min, d, sub);
                    link_state(p, Pos2::new(d_rect.right(), d_rect.center().y), Some(connected));
                }
            });
        if let Some(i) = forget {
            self.forget_output(i);
        }
        ui.label(
            RichText::new("Forgetting a profile unlinks the device. Flatline asks again next time you use it.")
                .font(body(12.5))
                .color(MIST2),
        );
        ui.spacing_mut().item_spacing.y = 12.0;
    }

    fn settings_sheet(&mut self, ui: &mut Ui, full: Rect) {
        let ctx = ui.ctx().clone();
        let w = 460.0;
        let rect =
            Rect::from_min_max(Pos2::new(full.right() - w - 16.0, full.top() + 16.0), full.max - Vec2::new(16.0, 16.0));
        egui::Area::new(egui::Id::unique("settings-sheet")).order(egui::Order::Foreground).fixed_pos(rect.min).show(
            &ctx,
            |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(24, 20, 26))
                    .stroke(Stroke::new(1.0, glass_edge()))
                    .corner_radius(CornerRadius::same(28))
                    .inner_margin(egui::Margin::same(24))
                    .show(ui, |ui| {
                        ui.set_width(w - 48.0);
                        ui.set_height(rect.height() - 48.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Settings").font(title(24.0)).color(MILK));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if widgets::glass_icon_button(ui, Icon::Close, 34.0).clicked() {
                                    self.settings_open = false;
                                }
                            });
                        });
                        ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| self.settings_body(ui));
                    });
            },
        );
    }

    fn settings_body(&mut self, ui: &mut Ui) {
        let w = ui.available_width();
        ui.spacing_mut().item_spacing.y = 12.0;
        ui.add_space(8.0);
        self.profiles_section(ui);
        ui.add_space(10.0);
        ui.label(widgets::eyebrow("SOUND", MIST2));
        let mut t = self.s.target;
        let rig = self.entry().map_or(profiles::TARGETS[0].name, |e| Catalog::get().target_name(e));
        egui::ComboBox::from_id_salt("target")
            .width(w)
            .selected_text(RichText::new(self.target_name()).font(body(15.0)).color(MILK))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut t, 0, rig).on_hover_text("The target matching the measurement rig (AutoEq)");
                if self.optimum_allowed() {
                    ui.selectable_value(&mut t, 1, profiles::TARGETS[1].name).on_hover_text(profiles::TARGETS[1].note);
                }
            });
        self.s.target = t;
        widgets::hslider(ui, "Tilt", &mut self.s.params.tilt_db_oct, -1.5..=1.5, 0.0, 0.05, |v| {
            format!("{v:+.2} dB/oct")
        });
        widgets::hslider(ui, "Max boost", &mut self.s.params.max_boost_db, 0.0..=12.0, 8.0, 0.5, |v| {
            format!("{v:.1} dB")
        });
        let mut bands = self.s.params.n_filters as f64;
        if widgets::hslider(ui, "Bands", &mut bands, 4.0..=16.0, 10.0, 1.0, |v| format!("{v:.0}")) {
            self.s.params.n_filters = bands as usize;
        }
        widgets::switch(ui, "Loudness-matched bypass", &mut self.s.level_match);

        ui.add_space(10.0);
        ui.label(widgets::eyebrow("OUTPUT", MIST2));
        let default_name = self
            .default_uid
            .as_ref()
            .and_then(|u| self.devices.iter().find(|d| &d.uid == u))
            .map_or("—".to_string(), |d| d.name.clone());
        let label = match &self.s.output_uid {
            None => format!("System default · {default_name}"),
            Some(u) => self.devices.iter().find(|d| &d.uid == u).map_or("(disconnected)".into(), |d| d.name.clone()),
        };
        let mut sel = self.s.output_uid.clone();
        egui::ComboBox::from_id_salt("output")
            .width(w)
            .selected_text(RichText::new(label).font(body(15.0)).color(MILK))
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
        ui.label(RichText::new("Buffer (frames) · lower = less latency").font(body(13.5)).color(MIST));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for b in BUFFER_SIZES {
                if widgets::pill(ui, &b.to_string(), self.s.buffer_frames == b).clicked() && self.s.buffer_frames != b {
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
            if on {
                self.start_engine();
            } else {
                self.stop_engine();
            }
        }
        let mut login = self.login_enabled;
        if widgets::switch(ui, "Launch at login (menu bar)", &mut login) {
            match crate::login::set_enabled(login) {
                Ok(()) => self.login_enabled = crate::login::is_enabled(),
                Err(e) => self.notify(format!("{e:#}")),
            }
        }

        ui.add_space(10.0);
        ui.label(widgets::eyebrow("CHART", MIST2));
        widgets::switch(ui, "Correction curve", &mut self.s.layers.eq);
        widgets::switch(ui, "Live analyzer", &mut self.s.layers.rta);

        ui.add_space(10.0);
        ui.label(widgets::eyebrow("EQ", MIST2));
        if let Some(d) = &self.design {
            for f in d.scaled_filters(self.s.amount) {
                ui.label(
                    RichText::new(format!(
                        "{:<4} {:>7} Hz  {:>+6.1} dB  Q {:.2}",
                        f.kind.short(),
                        graph::fmt_freq(f.freq),
                        f.gain_db,
                        f.q
                    ))
                    .font(mono(12.5))
                    .color(MIST),
                );
            }
        }
        ui.horizontal(|ui| {
            if secondary_button(ui, "Copy EQ").on_hover_text("Equalizer APO / AutoEq ParametricEQ format").clicked() {
                ui.ctx().copy_text(self.eq_text());
                self.notify("EQ copied to clipboard");
            }
            if secondary_button(ui, "Open log").clicked()
                && let Some(p) = crate::log::path()
            {
                let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "Flatline {} · {} measurements from {}",
                env!("CARGO_PKG_VERSION"),
                Catalog::get().len(),
                Catalog::get().provenance()
            ))
            .font(body(12.5))
            .color(MIST2),
        );
        let load = self.load * 100.0;
        ui.label(RichText::new(format!("DSP load {load:.2} % · space bar = A/B")).font(body(12.5)).color(MIST2));
    }

    // -------------------------------------------------------- overlays --

    pub(super) fn overlays(&mut self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop")));
            p.rect_filled(screen, CornerRadius::ZERO, alpha(NIGHT, 0.86));
            let r = screen.shrink(48.0);
            for s in egui::Shape::dashed_line(
                &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()],
                Stroke::new(2.0, alpha(MILK, 0.7)),
                12.0,
                9.0,
            ) {
                p.add(s);
            }
            p.text(
                screen.center() - Vec2::new(0.0, 16.0),
                Align2::CENTER_CENTER,
                "Drop to calibrate any headphone",
                title(30.0),
                MILK,
            );
            p.text(
                screen.center() + Vec2::new(0.0, 22.0),
                Align2::CENTER_CENTER,
                "A frequency-response CSV from AutoEq or REW",
                body(16.0),
                MIST,
            );
        }
        if let Some((msg, t, switched)) = &self.toast {
            let age = t.elapsed().as_secs_f32();
            if age > 3.2 {
                self.toast = None;
            } else {
                let a = (1.0 - ((age - 2.8) / 0.4).clamp(0.0, 1.0)) * (age / 0.15).min(1.0);
                let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("toast")));
                let galley = p.layout_no_wrap(msg.clone(), medium(15.5), alpha(MILK, a));
                let size = Vec2::new(galley.size().x + 74.0, 54.0);
                // On the chart, between its legend and the deviation number.
                let at = if self.onb.is_none() && self.chart_rect.is_positive() {
                    Pos2::new(self.chart_rect.center().x, self.chart_rect.top() + 36.0)
                } else {
                    Pos2::new(screen.center().x, screen.bottom() - 48.0)
                };
                let r = Rect::from_center_size(at, size);
                p.rect_filled(
                    r,
                    CornerRadius::same(27),
                    Color32::from_rgba_unmultiplied(30, 26, 32, (240.0 * a) as u8),
                );
                p.rect_stroke(
                    r,
                    CornerRadius::same(27),
                    Stroke::new(1.0, alpha(Color32::WHITE, 0.15 * a)),
                    StrokeKind::Inside,
                );
                let c = Pos2::new(r.left() + 28.0, r.center().y);
                if *switched {
                    p.circle_filled(c, 13.0, alpha(SOLEIL, a));
                    widgets::draw_icon(&p, Icon::Check, c, 15.0, alpha(NIGHT, a));
                } else {
                    p.circle_filled(c, 4.0, alpha(SOLEIL, a));
                }
                p.galley(Pos2::new(r.left() + 54.0, r.center().y - galley.size().y / 2.0), galley, MILK);
                ctx.request_repaint();
            }
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

/// "DT 770 PRO · worn pads" → "DT 770 PRO".
fn short_model(name: &str) -> String {
    name.split(" · ").next().unwrap_or(name).trim().to_owned()
}

/// Milk-white primary button.
pub(super) fn primary_button(ui: &mut Ui, text: &str) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_owned(), medium(15.0), NIGHT);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 44.0, 46.0), Sense::click());
    ui.painter().rect_filled(r, CornerRadius::same(23), if resp.hovered() { Color32::WHITE } else { MILK });
    ui.painter().galley(r.center() - g.size() / 2.0, g, NIGHT);
    resp
}

/// Glass secondary button.
pub(super) fn secondary_button(ui: &mut Ui, text: &str) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_owned(), medium(15.0), MILK);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 44.0, 46.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(23), alpha(Color32::WHITE, if resp.hovered() { 0.14 } else { 0.08 }));
    p.rect_stroke(r, CornerRadius::same(23), Stroke::new(1.0, alpha(Color32::WHITE, 0.2)), StrokeKind::Inside);
    p.galley(r.center() - g.size() / 2.0, g, MILK);
    resp
}

/// A profile row: headphones, their device (`linked`: `Some(connected)`), and
/// a check mark when selected. Rows whose device is not connected are dimmed.
fn headphone_row(ui: &mut Ui, name: &str, detail: &str, linked: Option<bool>, selected: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 66.0), Sense::click());
    let p = ui.painter();
    if selected || resp.hovered() {
        p.rect_filled(r, CornerRadius::same(16), alpha(Color32::WHITE, if selected { 0.09 } else { 0.05 }));
    }
    let away = linked == Some(false);
    let (ink, sub) = if away { (MIST2, alpha(MIST2, 0.8)) } else { (MILK, MIST) };
    let c = Pos2::new(r.left() + 30.0, r.center().y);
    p.circle_stroke(c, 19.0, Stroke::new(1.0, alpha(Color32::WHITE, 0.25)));
    widgets::draw_icon(p, Icon::Headphones, c, 18.0, ink);
    let x = r.left() + 64.0;
    p.text(Pos2::new(x, r.top() + 13.0), Align2::LEFT_TOP, name, medium(15.5), ink);
    let d = p.text(Pos2::new(x, r.top() + 36.0), Align2::LEFT_TOP, detail, body(13.0), sub);
    link_state(p, Pos2::new(d.right(), d.center().y), linked);
    if selected {
        let k = Pos2::new(r.right() - 26.0, r.center().y);
        p.line(
            vec![k + Vec2::new(-6.0, 0.0), k + Vec2::new(-1.0, 5.0), k + Vec2::new(7.0, -5.0)],
            Stroke::new(2.0, SOLEIL),
        );
    }
    resp
}

/// After a device name: a green dot and "connected", or "not connected".
fn link_state(p: &egui::Painter, at: Pos2, linked: Option<bool>) {
    match linked {
        Some(true) => {
            p.circle_filled(at + Vec2::new(10.0, 0.0), 3.0, CONNECTED);
            p.text(at + Vec2::new(18.0, 0.0), Align2::LEFT_CENTER, "connected", body(13.0), CONNECTED);
        }
        Some(false) => {
            p.text(at, Align2::LEFT_CENTER, " · not connected", body(13.0), alpha(MIST2, 0.8));
        }
        None => {}
    }
}

/// `text` on one line, cut with "…" to fit `width`.
fn clipped(
    p: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), egui::TextFormat::simple(font, color));
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(20.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    p.layout_job(job)
}
