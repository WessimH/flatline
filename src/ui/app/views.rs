//! The Horizon views: header, hero, chart card, control cards, the state
//! cards (bypass, paused, disconnected, output changed, blocked, error), the
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
        let device = self.engine.as_ref().map(|e| e.info.device.name.clone()).unwrap_or_default();
        let rate = self.engine.as_ref().map_or(0.0, |e| e.info.sample_rate / 1000.0);
        let hp_out = self.headphone_output_label();
        let (text, tone) = match self.status {
            Status::Live if self.s.enabled => (format!("Live · {device} · {rate:.0} kHz"), Tone::Live),
            Status::Live => (format!("Bypass · {device} · {rate:.0} kHz"), Tone::Neutral),
            Status::Starting => ("Starting…".to_owned(), Tone::Neutral),
            Status::Blocked => ("Blocked by macOS".to_owned(), Tone::Warning),
            Status::Error => ("Error".to_owned(), Tone::Error),
            Status::Standby => ("Off · system audio untouched".to_owned(), Tone::Neutral),
            Status::Asleep => ("Paused · Mac asleep".to_owned(), Tone::Neutral),
            Status::Disconnected => (format!("Disconnected · {hp_out}"), Tone::Warning),
            Status::OutputChanged => (format!("Paused · output is {}", self.current_output_name()), Tone::Warning),
        };
        let pill_w = ui.painter().layout_no_wrap(text.clone(), body(14.0), MILK).size().x + 50.0;
        let pill = Rect::from_center_size(Pos2::new(rect.center().x, rect.center().y), Vec2::new(pill_w, 40.0));
        ui.scope_builder(UiBuilder::new().max_rect(pill), |ui| {
            widgets::status_pill(ui, &text, tone, time);
        });
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

    // ------------------------------------------------------------ hero --

    fn hero(&mut self, ui: &mut Ui, time: f64) {
        let (model, variant) = self.headphone_label();
        let (l1, l2) = hero_title(&model);
        ui.spacing_mut().item_spacing.y = 14.0;
        let eyebrow = match self.entry() {
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
        ui.label(RichText::new(sub).font(light(16.0)).color(MIST));
        let default_bass = self.entry().map_or(6.0, |e| e.default_bass_db);
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
            let hp_out = self.headphone_output_label();
            let (t, sub) = match self.status {
                Status::Live | Status::Starting if self.s.enabled => {
                    ("Correction on", "Press Space to hear the difference".to_owned())
                }
                Status::Live | Status::Starting => {
                    ("Bypass", format!("You are hearing the raw {}", short_model(&hp_name)))
                }
                Status::Disconnected => ("Paused", format!("Waiting for {hp_out}")),
                Status::OutputChanged => ("Paused", format!("Output is {}", self.current_output_name())),
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
        paint_glass(ui.painter(), rect, 30);
        let inner = rect.shrink2(Vec2::new(26.0, 22.0));
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
            let head = Rect::from_min_size(inner.min, Vec2::new(inner.width(), 70.0));
            let p = ui.painter();
            p.text(head.left_top(), Align2::LEFT_TOP, "Frequency response", medium(17.0), MILK);
            let name = self.headphone_name();
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
            Status::Disconnected if !self.pause_card_dismissed => self.disconnected_card(ui, rect),
            Status::OutputChanged if !self.pause_card_dismissed => self.output_changed_card(ui, rect),
            Status::Error => self.error_card(ui, rect),
            Status::Asleep => self.asleep_card(ui, rect),
            _ => {}
        }
    }

    fn state_card(
        ui: &mut Ui,
        area: Rect,
        icon: Icon,
        color: Color32,
        title_text: &str,
        body_text: &str,
        add: impl FnOnce(&mut Ui),
    ) {
        // Centred in the plot, below the chart header and legend.
        let plot = Rect::from_min_max(area.min + Vec2::new(0.0, 92.0), area.max);
        let card = Rect::from_center_size(plot.center(), Vec2::new((area.width() - 120.0).min(540.0), 344.0));
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

    fn disconnected_card(&mut self, ui: &mut Ui, area: Rect) {
        let hp = self.headphone_output_label();
        let now = self.current_output_name();
        let wired = self.current_output.as_ref().filter(|d| d.wired).map(|d| d.uid.clone());
        let (mut dismiss, mut here) = (false, false);
        Self::state_card(
            ui,
            area,
            Icon::Unplug,
            AMBER,
            "Your headphones are disconnected",
            &format!("Correction is paused. Plug {hp} back in and Flatline resumes on its own."),
            |ui| {
                if wired.is_some() {
                    here = primary_button(ui, "My headphones are plugged in here").clicked();
                }
                if !now.is_empty() && secondary_button(ui, &format!("Use {now} without correction")).clicked() {
                    dismiss = true;
                }
            },
        );
        if here && let Some(uid) = wired {
            // route() sees a known output again and resumes.
            self.s.remember_output(&uid, &now);
        }
        if dismiss {
            self.pause_card_dismissed = true;
        }
    }

    fn output_changed_card(&mut self, ui: &mut Ui, area: Rect) {
        let name = self.headphone_name();
        let now = self.current_output_name();
        let (mut switch, mut keep, mut use_here) = (false, false, false);
        Self::state_card(
            ui,
            area,
            Icon::Headphones,
            AMBER,
            &format!("Output switched to {now}"),
            &format!("Your {} profile doesn't match these headphones. Correction is paused.", short_model(&name)),
            |ui| {
                ui.horizontal(|ui| {
                    switch = primary_button(ui, "Switch profile").clicked();
                    use_here = secondary_button(ui, "Correct here anyway").clicked();
                    keep = secondary_button(ui, "Keep paused").clicked();
                });
            },
        );
        if switch {
            self.switcher_open = true;
        }
        if use_here && let Some(uid) = self.default_uid.clone() {
            // The headphones are on this output too.
            self.s.remember_output(&uid, &now);
        }
        if keep {
            self.pause_card_dismissed = true;
        }
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
            area,
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
            area,
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

    fn switcher(&mut self, ui: &mut Ui, hero_left: Rect) {
        let ctx = ui.ctx().clone();
        let pos = hero_left.left_top() + Vec2::new(0.0, 210.0);
        let mut pick: Option<String> = None;
        let (mut close, mut add_new) = (false, false);
        let resp = egui::Area::new(egui::Id::unique("switcher")).order(egui::Order::Foreground).fixed_pos(pos).show(
            &ctx,
            |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(28, 24, 30))
                    .stroke(Stroke::new(1.0, glass_edge()))
                    .corner_radius(CornerRadius::same(24))
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_width(380.0);
                        ui.label(widgets::eyebrow("MY HEADPHONES", MIST));
                        ui.add_space(4.0);
                        let cat = Catalog::get();
                        let mut rows: Vec<(String, String, String)> = self
                            .s
                            .my_headphones
                            .iter()
                            .filter_map(|k| cat.find(k))
                            .map(|e| {
                                let name = match e.variant {
                                    Some(v) => format!("{} · {v}", e.model),
                                    None => e.model.to_owned(),
                                };
                                (e.key(), name, format!("{} · measured by {}", e.brand, e.source))
                            })
                            .collect();
                        if let Some(c) = &self.s.custom {
                            rows.push((CUSTOM_KEY.to_owned(), c.name.clone(), "Your measurement (CSV)".into()));
                        }
                        for (key, name, detail) in rows {
                            if headphone_row(ui, &name, &detail, key == self.s.headphone_key).clicked() {
                                pick = Some(key);
                            }
                        }
                        ui.add_space(4.0);
                        let (r, add) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::click());
                        let p = ui.painter();
                        if add.hovered() {
                            p.rect_filled(r, CornerRadius::same(14), alpha(Color32::WHITE, 0.06));
                        }
                        let c = Pos2::new(r.left() + 26.0, r.center().y);
                        p.circle_stroke(c, 17.0, Stroke::new(1.0, alpha(Color32::WHITE, 0.3)));
                        widgets::draw_icon(p, Icon::Plus, c, 18.0, MILK);
                        p.text(
                            Pos2::new(r.left() + 56.0, r.center().y),
                            Align2::LEFT_CENTER,
                            "Add headphones…",
                            medium(15.0),
                            MILK,
                        );
                        if add.clicked() {
                            add_new = true;
                        }
                    });
            },
        );
        if let Some(k) = pick {
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

    fn settings_sheet(&mut self, ui: &mut Ui, full: Rect) {
        let ctx = ui.ctx().clone();
        let w = 400.0;
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
            if let Some(d) = sel.as_ref().and_then(|u| self.devices.iter().find(|d| &d.uid == u)) {
                // An explicit choice is an output the headphones are on.
                self.s.remember_output(&d.uid, &d.name);
            }
            self.s.output_uid = sel;
            if self.s.engine_on {
                self.start_engine();
            }
        }
        let mut forget = None;
        for (i, o) in self.s.headphone_outputs.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Headphones on {}", o.name)).font(body(13.5)).color(MIST));
                if widgets::tag(ui, "Forget").on_hover_text("Remove this output from the headphones' outputs").clicked()
                {
                    forget = Some(i);
                }
            });
        }
        if let Some(i) = forget {
            self.s.headphone_outputs.remove(i);
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
        if let Some((msg, t)) = &self.toast {
            let age = t.elapsed().as_secs_f32();
            if age > 3.2 {
                self.toast = None;
            } else {
                let a = (1.0 - ((age - 2.8) / 0.4).clamp(0.0, 1.0)) * (age / 0.15).min(1.0);
                let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("toast")));
                let galley = p.layout_no_wrap(msg.clone(), body(15.0), alpha(MILK, a));
                let size = galley.size() + Vec2::new(56.0, 26.0);
                let r = Rect::from_center_size(Pos2::new(screen.center().x, screen.bottom() - 48.0), size);
                p.rect_filled(
                    r,
                    CornerRadius::same(24),
                    Color32::from_rgba_unmultiplied(30, 26, 32, (240.0 * a) as u8),
                );
                p.rect_stroke(
                    r,
                    CornerRadius::same(24),
                    Stroke::new(1.0, alpha(Color32::WHITE, 0.15 * a)),
                    StrokeKind::Inside,
                );
                p.circle_filled(Pos2::new(r.left() + 22.0, r.center().y), 4.0, alpha(SOLEIL, a));
                p.galley(Pos2::new(r.left() + 38.0, r.center().y - galley.size().y / 2.0), galley, MILK);
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

/// A row of the headphone switcher.
fn headphone_row(ui: &mut Ui, name: &str, detail: &str, selected: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 60.0), Sense::click());
    let p = ui.painter();
    if selected || resp.hovered() {
        p.rect_filled(r, CornerRadius::same(14), alpha(Color32::WHITE, if selected { 0.09 } else { 0.05 }));
    }
    let c = Pos2::new(r.left() + 26.0, r.center().y);
    p.circle_stroke(c, 17.0, Stroke::new(1.0, alpha(Color32::WHITE, 0.25)));
    widgets::draw_icon(p, Icon::Headphones, c, 18.0, MILK);
    p.text(Pos2::new(r.left() + 56.0, r.top() + 12.0), Align2::LEFT_TOP, name, medium(15.0), MILK);
    p.text(Pos2::new(r.left() + 56.0, r.top() + 33.0), Align2::LEFT_TOP, detail, body(12.5), MIST2);
    if selected {
        let k = Pos2::new(r.right() - 22.0, r.center().y);
        p.line(
            vec![k + Vec2::new(-6.0, 0.0), k + Vec2::new(-1.0, 5.0), k + Vec2::new(7.0, -5.0)],
            Stroke::new(2.0, SOLEIL),
        );
    }
    resp
}
