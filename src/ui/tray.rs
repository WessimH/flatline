//! Menu-bar icon: lets Flatline keep correcting with its window closed.

use eframe::egui;
use std::sync::mpsc;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Debug, Clone, Copy, PartialEq)]
/// A click in the menu-bar menu.
pub enum TrayAction {
    /// Open the window.
    Show,
    /// Toggle the correction (A/B).
    ToggleCorrection,
    /// Quit Flatline.
    Quit,
}

/// The menu-bar icon and its menu.
pub struct Tray {
    _icon: TrayIcon,
    status: MenuItem,
    correction: CheckMenuItem,
    rx: mpsc::Receiver<TrayAction>,
    shown: (bool, String),
}

impl Tray {
    /// Creates the menu-bar icon; `None` if the platform refuses.
    pub fn new(ctx: egui::Context, correction_on: bool) -> Option<Self> {
        let menu = Menu::new();
        let status = MenuItem::new("Flatline", false, None);
        let correction = CheckMenuItem::new("Correction", true, correction_on, None);
        let show = MenuItem::new("Open Flatline…", true, None);
        let quit = MenuItem::new("Quit Flatline", true, None);
        menu.append(&status).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&correction).ok()?;
        menu.append(&show).ok()?;
        menu.append(&PredefinedMenuItem::separator()).ok()?;
        menu.append(&quit).ok()?;

        let ids = (correction.id().clone(), show.id().clone(), quit.id().clone());
        let (tx, rx) = mpsc::channel();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let action = if e.id == ids.0 {
                TrayAction::ToggleCorrection
            } else if e.id == ids.1 {
                TrayAction::Show
            } else if e.id == ids.2 {
                TrayAction::Quit
            } else {
                return;
            };
            let _ = tx.send(action);
            // Wake the app even while its window is hidden.
            ctx.request_repaint();
        }));

        let px = 44;
        let icon = Icon::from_rgba(super::icon::render_template(px), px, px).ok()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon_templated(icon)
            .with_tooltip("Flatline")
            .build()
            .ok()?;
        Some(Self { _icon: tray, status, correction, rx, shown: (correction_on, String::new()) })
    }

    /// Menu clicks since the last call.
    pub fn actions(&self) -> Vec<TrayAction> {
        self.rx.try_iter().collect()
    }

    /// Keep the menu in sync with the app state (cheap when nothing changed).
    pub fn sync(&mut self, correction_on: bool, status: &str) {
        if self.shown.0 != correction_on || self.correction.is_checked() != correction_on {
            self.correction.set_checked(correction_on);
            self.shown.0 = correction_on;
        }
        if self.shown.1 != status {
            self.status.set_text(status);
            self.shown.1 = status.to_owned();
        }
    }
}

/// Show / hide the Dock icon (menu-bar-only mode while the window is closed).
pub fn set_dock_visible(visible: bool) {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    });
    if visible {
        app.activate();
    }
}
