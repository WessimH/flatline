//! "Launch at login" via a per-user LaunchAgent that starts the bundle hidden
//! in the menu bar (`--background`). Going through `open` keeps the launch
//! attributed to Flatline.app, which is what the system-audio permission needs.

use anyhow::{Result, anyhow};
use std::path::PathBuf;

const LABEL: &str = "dev.flatline.app.login";

fn plist_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

/// Path of the running .app bundle, if we were launched from one.
pub fn bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(PathBuf::from)
}

/// Whether launch at login is enabled.
pub fn is_enabled() -> bool {
    plist_path().is_some_and(|p| p.exists())
}

/// Enables or disables launch at login (needs to run from `Flatline.app`).
pub fn set_enabled(on: bool) -> Result<()> {
    let path = plist_path().ok_or_else(|| anyhow!("no HOME"))?;
    if !on {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    let app = bundle_path().ok_or_else(|| anyhow!("launch-at-login needs Flatline.app (run scripts/bundle.sh)"))?;
    let app = app.to_string_lossy().replace('&', "&amp;").replace('<', "&lt;");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>AssociatedBundleIdentifiers</key><string>dev.flatline.app</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/bin/open</string>
    <string>-g</string>
    <string>-a</string>
    <string>{app}</string>
    <string>--args</string>
    <string>--background</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#
    );
    if std::fs::read_to_string(&path).ok().as_deref() != Some(plist.as_str()) {
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, plist)?;
    }
    Ok(())
}
