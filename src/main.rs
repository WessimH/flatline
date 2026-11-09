//! The `flatline` binary: the GUI app, plus a few diagnostic subcommands.
//!
//! ```text
//! flatline                    run the app
//! flatline --background       start hidden in the menu bar (used at login)
//! flatline --devices          list audio outputs (* = system default)
//! flatline --print-eq         print the DT 770 PRO correction (Equalizer APO format)
//! flatline --dump-curves      measured / target / EQ / result curves as JSON
//! flatline --headless [secs]  run the engine without UI and print levels
//! flatline --write-iconset D  write the app icon as a macOS .iconset
//! ```

use flatline::{dsp, engine, profiles, ui};
use std::sync::{Arc, atomic::Ordering::Relaxed};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);

    if has("--devices") {
        let def = engine::default_output_uid();
        for d in engine::list_output_devices() {
            let mark = if Some(&d.uid) == def.as_ref() { "*" } else { " " };
            println!("{mark} {:<40} {}", d.name, d.uid);
        }
        return Ok(());
    }
    if has("--print-eq") {
        let hp = profiles::HEADPHONES[0].curve();
        let tg = profiles::TARGETS[0].curve();
        let d = dsp::fit::design(&hp, &tg, &Default::default(), 48_000.0);
        println!("{}: deviation {:.2} → {:.2} dB", profiles::HEADPHONES[0].name, d.error_before_db, d.error_after_db);
        println!("Preamp: {:.1} dB", -dsp::fit::max_gain_db(&d.filters, 48_000.0).max(0.0));
        for (i, f) in d.filters.iter().enumerate() {
            println!(
                "Filter {}: ON {} Fc {:.0} Hz Gain {:.1} dB Q {:.2}",
                i + 1,
                f.kind.short(),
                f.freq,
                f.gain_db,
                f.q
            );
        }
        return Ok(());
    }
    if has("--dump-curves") {
        // 1/12-octave curves as JSON (design mockups, debugging).
        let hp = profiles::HEADPHONES[0].curve();
        let tg = profiles::TARGETS[0].curve();
        let d = dsp::fit::design(&hp, &tg, &Default::default(), 48_000.0);
        let eq = dsp::biquad::cascade_response(&d.filters, &d.grid, 48_000.0);
        let pts: Vec<String> = (0..d.grid.len())
            .step_by(4)
            .map(|i| {
                format!(
                    "[{:.1},{:.2},{:.2},{:.2},{:.2}]",
                    d.grid[i],
                    d.measured[i],
                    d.target[i],
                    eq[i],
                    d.measured[i] + eq[i]
                )
            })
            .collect();
        println!("[{}]", pts.join(","));
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--write-iconset") {
        let dir = args.get(i + 1).ok_or_else(|| anyhow::anyhow!("--write-iconset <dir>"))?;
        ui::icon::write_iconset(std::path::Path::new(dir))?;
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--headless") {
        return headless(args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(5.0));
    }

    // Two instances would tap each other's output (feedback): allow only one.
    let _lock = match single_instance() {
        Some(l) => l,
        None => {
            eprintln!("Flatline is already running.");
            return Ok(());
        }
    };

    // FL_WINDOW=1920x1200: open the window at this size (testing).
    let size = std::env::var("FL_WINDOW")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some([w.parse().ok()?, h.parse().ok()?])))
        .unwrap_or([1320.0, 880.0]);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Flatline")
            .with_app_id("dev.flatline.app")
            .with_inner_size(size)
            .with_min_inner_size([1060.0, 700.0])
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_icon(Arc::new(ui::icon::window_icon()))
            // Launched at login: start hidden in the menu bar.
            .with_visible(!has("--background")),
        // A scenario (FL_SCENARIO) leaves the settings file untouched.
        persist_window: std::env::var_os("FL_SCENARIO").is_none() && std::env::var_os("FL_WINDOW").is_none(),
        ..Default::default()
    };
    eframe::run_native("Flatline", options, Box::new(|cc| Ok(Box::new(ui::app::FlatlineApp::new(cc)))))
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn single_instance() -> Option<std::fs::File> {
    let dir = std::path::PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Flatline");
    std::fs::create_dir_all(&dir).ok()?;
    let f =
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("instance.lock")).ok()?;
    f.try_lock().ok()?;
    Some(f)
}

/// Runs the engine without UI and prints levels (diagnostics).
/// Note: macOS only grants system-audio capture to the GUI app, so this mode
/// is mostly useful to check device setup.
fn headless(secs: f64) -> anyhow::Result<()> {
    let shared = Arc::new(engine::Shared::default());
    let hp = profiles::HEADPHONES[0].curve();
    let tg = profiles::TARGETS[0].curve();
    let bf: u32 = std::env::var("FL_BUF").ok().and_then(|v| v.parse().ok()).unwrap_or(256);
    let mut eng = engine::Engine::start(None, bf, true, shared.clone(), |fs| {
        let d = dsp::fit::design(&hp, &tg, &Default::default(), fs);
        let pre = -dsp::fit::max_gain_db(&d.filters, fs).max(0.0);
        dsp::processor::EqSettings::from_filters(&d.filters, pre, pre, fs)
    })?;
    println!("running: {:?} is_running={}", eng.info, eng.is_running());
    for _ in 0..(secs * 2.0) as usize {
        std::thread::sleep(std::time::Duration::from_millis(500));
        while eng.scope.pop().is_ok() {}
        println!(
            "cb {:>6}  frames {:>4}  in {:.3}/{:.3}  out {:.3}/{:.3}  load {:.1}%  io_offset {:.0}",
            shared.callbacks.load(Relaxed),
            shared.frames.load(Relaxed),
            shared.peak_in[0].take(),
            shared.peak_in[1].take(),
            shared.peak_out[0].take(),
            shared.peak_out[1].take(),
            shared.load.take() * 100.0,
            shared.io_offset.load(),
        );
    }
    Ok(())
}
