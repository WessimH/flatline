//! Simulated audio outputs for tests and screenshots (`FL_SCENARIO`).
//!
//! A scenario lists the outputs at launch and timed steps that add or remove
//! outputs, change the default output or forget a profile. While a scenario
//! runs, the app reads its outputs from [`Sim`] instead of CoreAudio and runs
//! a fake engine: no tap, no audio, no permission prompt.
//!
//! ```ron
//! (
//!     settings: Some((onboarded: true, version: 6)),
//!     devices: [("BuiltInSpeakerDevice", "MacBook Pro Speakers")],
//!     default: Some("BuiltInSpeakerDevice"),
//!     steps: [(at: 3, events: [Add("usb-1", "Scarlett Solo USB"), Default(Some("usb-1"))])],
//! )
//! ```

use super::Settings;
use crate::engine::Device;
use serde::Deserialize;
use std::time::{Duration, Instant};

/// Shortest gap between two steps, and before the first one. After a change
/// of output the engine restarts within 2 s; a screenshot is taken 0.5 s
/// before each step and must show the settled state.
const MIN_GAP: u64 = 3;

/// A scenario file (RON).
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scenario {
    /// Settings at launch (missing fields take their default). They are not
    /// migrated: write `version: 6`. `None`: default settings, set up.
    pub settings: Option<Settings>,
    /// Outputs connected at launch: `(uid, name)`.
    pub devices: Vec<(String, String)>,
    /// UID of the system default output at launch.
    pub default: Option<String>,
    /// Changes, in time order.
    pub steps: Vec<Step>,
}

/// Changes that happen together, `at` seconds after launch.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Seconds after launch.
    pub at: u64,
    /// What changes, in order.
    pub events: Vec<Event>,
}

/// One change of a [`Step`].
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub enum Event {
    /// An output is connected: `(uid, name)`.
    Add(String, String),
    /// An output is disconnected (no longer the default if it was).
    Remove(String),
    /// The system default output changes (`None`: no default output).
    Default(Option<String>),
    /// The user clicks "Forget" on the profile of this output.
    Forget(String),
}

impl Scenario {
    /// Reads a scenario and checks the gaps between its steps.
    pub fn parse(text: &str) -> Result<Self, String> {
        let sc: Self = ron::from_str(text).map_err(|e| e.to_string())?;
        let mut prev = 0;
        for (i, step) in sc.steps.iter().enumerate() {
            if step.at < prev + MIN_GAP {
                return Err(format!(
                    "step {} at {} s: steps must be at least {MIN_GAP} s apart, the first at {MIN_GAP} s or later",
                    i + 1,
                    step.at
                ));
            }
            prev = step.at;
        }
        Ok(sc)
    }
}

/// The simulated outputs while a scenario runs, and the fake engine.
pub struct Sim {
    steps: Vec<Step>,
    start: Instant,
    next: usize,
    /// Connected outputs.
    pub devices: Vec<Device>,
    /// UID of the system default output.
    pub default: Option<String>,
    /// The output the fake engine plays on (`None`: stopped).
    pub engine: Option<Device>,
}

impl Sim {
    /// Reads the scenario file at `path`.
    pub fn load(path: &str) -> Result<Scenario, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        Scenario::parse(&text).map_err(|e| format!("{path}: {e}"))
    }

    /// Starts a scenario at `start`. Returns its settings (default ones,
    /// set up, if it has none) and the simulation.
    pub fn new(sc: Scenario, start: Instant) -> (Settings, Self) {
        let settings = sc.settings.unwrap_or(Settings { onboarded: true, ..Settings::default() });
        let sim = Self {
            steps: sc.steps,
            start,
            next: 0,
            devices: sc.devices.into_iter().map(|(uid, name)| Device { id: 0, uid, name }).collect(),
            default: sc.default,
            engine: None,
        };
        (settings, sim)
    }

    /// When each step happens, after launch.
    pub fn times(&self) -> impl Iterator<Item = Duration> + '_ {
        self.steps.iter().map(|s| Duration::from_secs(s.at))
    }

    /// Applies the steps due at `now`. Returns the UIDs whose profile the
    /// user forgets.
    pub fn advance(&mut self, now: Instant) -> Vec<String> {
        let mut forget = Vec::new();
        while let Some(step) = self.steps.get(self.next).filter(|s| now >= self.start + Duration::from_secs(s.at)) {
            let step = step.clone();
            self.next += 1;
            crate::log::log(format!("scenario step {} ({} s): {:?}", self.next, step.at, step.events));
            for event in step.events {
                match event {
                    Event::Add(uid, name) => {
                        self.devices.retain(|d| d.uid != uid);
                        self.devices.push(Device { id: 0, uid, name });
                    }
                    Event::Remove(uid) => {
                        self.devices.retain(|d| d.uid != uid);
                        if self.default.as_ref() == Some(&uid) {
                            self.default = None;
                        }
                    }
                    Event::Default(uid) => self.default = uid,
                    Event::Forget(uid) => forget.push(uid),
                }
            }
        }
        forget
    }
}

#[cfg(test)]
mod tests {
    use super::super::{DEFAULT_KEY, FlatlineApp, Status};
    use super::*;
    use eframe::egui;

    const SCARLETT: &str = "AppleUSBAudioEngine:Focusrite:Scarlett Solo USB:1";
    const AIRPODS_KEY: &str = "Rtings/HMS II.3/Apple AirPods Pro";

    /// What a test sees at one time: status, headphones, banner, new log lines.
    struct Seen {
        status: Status,
        headphones: String,
        invite: Option<String>,
        log: Vec<String>,
    }

    /// Runs a scenario with a fake clock, polling every 250 ms like the app,
    /// and reports the state just before each step and at the end.
    fn replay(text: &str) -> Vec<Seen> {
        let sc = Scenario::parse(text).expect("scenario parses");
        let t0 = Instant::now();
        let (s, sim) = Sim::new(sc, t0);
        let ends: Vec<Duration> = sim
            .times()
            .map(|t| t - Duration::from_millis(500))
            .chain([sim.times().last().unwrap_or_default() + Duration::from_secs(3)])
            .collect();
        crate::log::take_captured();
        let ctx = egui::Context::default();
        let mut app = FlatlineApp::build(&ctx, s, true, Some(sim), None, Default::default(), t0);
        let (mut seen, mut t) = (Vec::new(), Duration::ZERO);
        for end in ends {
            while t + Duration::from_millis(250) <= end {
                t += Duration::from_millis(250);
                app.poll(t0 + t);
            }
            seen.push(Seen {
                status: app.status,
                headphones: app.s.headphone_key.clone(),
                invite: app.invite.clone(),
                log: crate::log::take_captured(),
            });
        }
        seen
    }

    fn has(log: &[String], text: &str) -> bool {
        log.iter().any(|l| l.contains(text))
    }

    #[test]
    fn unplugging_pauses_and_plugging_back_resumes() {
        let seen = replay(include_str!("scenarios/unplug.ron"));
        assert_eq!(seen[0].status, Status::Live);
        assert!(has(&seen[0].log, "engine started: Scarlett Solo USB (AppleUSBAudioEngine"), "{:?}", seen[0].log);
        assert_eq!(seen[1].status, Status::Speakers);
        assert!(has(&seen[1].log, "built-in speakers: no correction"), "{:?}", seen[1].log);
        assert_eq!(seen[2].status, Status::Live);
        assert!(has(&seen[2].log, "left the built-in speakers: resuming"), "{:?}", seen[2].log);
        assert!(seen.iter().all(|s| s.headphones == DEFAULT_KEY && s.invite.is_none()));
    }

    #[test]
    fn airpods_load_their_profile_and_back() {
        assert!(crate::catalog::Catalog::get().find(AIRPODS_KEY).is_some());
        let seen = replay(include_str!("scenarios/airpods.ron"));
        assert_eq!((seen[0].status, seen[0].headphones.as_str()), (Status::Live, DEFAULT_KEY));
        assert_eq!((seen[1].status, seen[1].headphones.as_str()), (Status::Live, AIRPODS_KEY));
        assert!(has(&seen[1].log, &format!("headphones {AIRPODS_KEY}")), "{:?}", seen[1].log);
        assert!(has(&seen[1].log, "restarting engine: output moved to 00-11-22-33-44-55:output"));
        assert_eq!((seen[2].status, seen[2].headphones.as_str()), (Status::Live, DEFAULT_KEY));
        assert!(seen.iter().all(|s| s.invite.is_none()));
    }

    #[test]
    fn speakers_are_not_corrected() {
        let seen = replay(include_str!("scenarios/speakers.ron"));
        assert_eq!(seen[0].status, Status::Live);
        assert_eq!((seen[1].status, seen[1].headphones.as_str()), (Status::Speakers, DEFAULT_KEY));
        assert!(has(&seen[1].log, "built-in speakers: no correction"));
    }

    #[test]
    fn unknown_outputs_ask_for_their_headphones() {
        let seen = replay(include_str!("scenarios/unknown.ron"));
        assert_eq!(seen[0].invite, None);
        assert_eq!((seen[1].status, seen[1].invite.as_deref()), (Status::Live, Some("AA-BB-CC-DD-EE-FF:output")));
        // Same name as the known Scarlett, other UID: a new output too.
        let second = "AppleUSBAudioEngine:Focusrite:Scarlett Solo USB:2";
        assert_eq!((seen[2].status, seen[2].invite.as_deref()), (Status::Live, Some(second)));
        assert!(seen.iter().all(|s| s.headphones == DEFAULT_KEY));
    }

    #[test]
    fn forgetting_the_playing_output_asks_again() {
        let seen = replay(include_str!("scenarios/forget.ron"));
        assert_eq!(seen[0].invite, None);
        assert_eq!((seen[1].status, seen[1].invite.as_deref()), (Status::Live, Some(SCARLETT)));
    }

    #[test]
    fn a_scenario_replays_the_same_log() {
        let text = include_str!("scenarios/airpods.ron");
        let logs = |seen: Vec<Seen>| seen.into_iter().flat_map(|s| s.log).collect::<Vec<_>>();
        assert_eq!(logs(replay(text)), logs(replay(text)));
    }

    #[test]
    fn no_output_is_a_neutral_state_not_an_error() {
        for text in ["()", include_str!("scenarios/no-output.ron")] {
            let seen = replay(text);
            assert_eq!(seen[0].status, Status::NoOutput, "{:?}", seen[0].log);
            assert!(has(&seen[0].log, "no output device: waiting for one"), "{:?}", seen[0].log);
            assert!(!has(&seen[0].log, "engine failed"), "{:?}", seen[0].log);
        }
    }

    #[test]
    fn an_output_appearing_starts_the_engine() {
        let seen = replay(include_str!("scenarios/output-appears.ron"));
        assert_eq!(seen[1].status, Status::Live, "{:?}", seen[1].log);
        assert_eq!(seen[1].headphones, DEFAULT_KEY);
        assert!(has(&seen[1].log, "restarting engine: an output is available"), "{:?}", seen[1].log);
        let second = "AppleUSBAudioEngine:Focusrite:Scarlett Solo USB:2";
        assert_eq!((seen[3].status, seen[3].invite.as_deref()), (Status::Live, Some(second)));
    }

    #[test]
    fn an_engine_error_is_retried_spaced_out() {
        let t0 = Instant::now();
        let sc = Scenario::parse(include_str!("scenarios/speakers.ron")).unwrap();
        let (s, sim) = Sim::new(Scenario { steps: Vec::new(), ..sc }, t0);
        let ctx = egui::Context::default();
        let mut app = FlatlineApp::build(&ctx, s, true, Some(sim), None, Default::default(), t0);
        app.poll(t0);
        assert_eq!(app.status, Status::Live);
        // The engine failed to start (e.g. the output is hogged).
        app.drop_engine();
        app.status = Status::Error;
        crate::log::take_captured();
        let mut retries = Vec::new();
        for i in 1..=4 * 80 {
            let t = Duration::from_millis(250 * i);
            app.poll(t0 + t);
            if crate::log::take_captured().iter().any(|l| l.contains("restarting engine: still failing")) {
                retries.push(t.as_secs());
                app.drop_engine();
                app.status = Status::Error;
            }
        }
        // 10 s after the failure, then 20 s after the first retry, …
        assert_eq!(retries, [10, 30, 70]);
    }

    #[test]
    fn bad_scenarios_say_why() {
        let close = "(steps: [(at: 3, events: []), (at: 4, events: [])])";
        assert!(Scenario::parse(close).unwrap_err().contains("step 2 at 4 s"));
        assert!(Scenario::parse("(steps: [(at: 1, events: [])])").unwrap_err().contains("step 1 at 1 s"));
        assert!(Scenario::parse("(steps: [(at: 3, events: [Explode(\"x\")])])").unwrap_err().contains("Explode"));
        assert!(Scenario::parse("(devices: [").is_err());
        assert!(Scenario::parse("(device: [])").unwrap_err().contains("device"));
        assert!(Sim::load("/nonexistent/flatline.ron").unwrap_err().starts_with("/nonexistent/flatline.ron: "));
    }
}
