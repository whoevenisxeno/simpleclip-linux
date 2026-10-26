//! The tray icon (a StatusNotifierItem, so it works in KDE, GNOME with the
//! AppIndicator extension, waybar, and every other bar that has a tray).

use crate::state::Daemon;
use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, MenuItem, StandardItem};
use ksni::{Icon, Tray};
use sc_core::capture::CaptureState;
use std::sync::mpsc::Sender;
use std::time::Duration;

/// The app logo at 32x32 (assets/tray-32.rgba). While paused it is drawn in
/// grey and slightly faded, so the state shows at a glance. Returned as the
/// ARGB32 the tray protocol wants.
fn tray_icon_image(active: bool) -> Icon {
    const N: i32 = 32;
    static LOGO: &[u8] = include_bytes!("../../../assets/tray-32.rgba");
    let mut data = Vec::with_capacity(LOGO.len());
    for px in LOGO.as_chunks::<4>().0 {
        let (mut r, mut g, mut b, mut a) = (px[0], px[1], px[2], px[3]);
        if !active {
            let grey = (r as f32 * 0.30 + g as f32 * 0.59 + b as f32 * 0.11) as u8;
            (r, g, b, a) = (grey, grey, grey, (a as f32 * 0.7) as u8);
        }
        data.extend_from_slice(&[a, r, g, b]);
    }
    Icon {
        width: N,
        height: N,
        data,
    }
}

struct SimpleClipTray {
    daemon: Daemon,
    paused: bool,
    quit: Sender<()>,
}

impl SimpleClipTray {
    fn action(&self, id: &'static str) -> Box<dyn Fn(&mut Self) + Send + 'static> {
        Box::new(move |t: &mut Self| {
            let d = t.daemon.clone();
            std::thread::spawn(move || crate::actions::run(&d, id));
        })
    }
}

impl Tray for SimpleClipTray {
    fn id(&self) -> String {
        "simpleclip".into()
    }
    fn title(&self) -> String {
        "SimpleClip".into()
    }
    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![tray_icon_image(!self.paused)]
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: if self.paused {
                "SimpleClip - paused"
            } else {
                "SimpleClip - buffering"
            }
            .into(),
            ..Default::default()
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let d = self.daemon.clone();
        std::thread::spawn(move || crate::actions::run(&d, "open"));
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Save clip now".into(),
                activate: self.action("save"),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Take screenshot".into(),
                activate: self.action("screenshot"),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Open SimpleClip".into(),
                activate: self.action("open"),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Open clips folder".into(),
                activate: Box::new(|t: &mut Self| {
                    crate::lin::open_folder(&t.daemon.config.read().unwrap().save.resolve_dir());
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: if self.paused {
                    "Resume buffering"
                } else {
                    "Pause buffering"
                }
                .into(),
                activate: Box::new(|t: &mut Self| {
                    let paused = t.daemon.capture_state() != CaptureState::Paused;
                    t.daemon.set_paused(paused);
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Start at login".into(),
                checked: crate::lin::autostart_enabled(),
                activate: Box::new(|_t: &mut Self| {
                    // Persist through the config so the setting survives restarts
                    // (the daemon re-applies it from there).
                    let on = !crate::lin::autostart_enabled();
                    crate::lin::set_autostart(on);
                    if let Ok(path) = sc_core::config::Config::default_path() {
                        if let Ok(mut cfg) = sc_core::config::Config::load(&path) {
                            cfg.general.autostart = on;
                            let _ = cfg.save(&path);
                        }
                    }
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.quit.send(());
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Run the tray icon until the user picks Quit. Returns when they do. Without a
/// tray host (no bar with a tray) the daemon just carries on without an icon,
/// and this blocks forever.
pub fn run(daemon: Daemon) {
    let (quit_tx, quit_rx) = std::sync::mpsc::channel();
    let tray = SimpleClipTray {
        daemon: daemon.clone(),
        paused: false,
        quit: quit_tx,
    };
    let handle = match tray.assume_sni_available(true).spawn() {
        Ok(h) => Some(h),
        Err(e) => {
            tracing::warn!(error = %e, "no tray available; running without an icon");
            None
        }
    };
    loop {
        match quit_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(()) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            // No tray: nothing can ever send Quit, so just keep the daemon alive.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => loop {
                std::thread::sleep(Duration::from_secs(3600));
            },
        }
        // Reflect pause state however it changed (menu, `sc pause`, or the GUI).
        if let Some(h) = &handle {
            let paused = daemon.capture_state() == CaptureState::Paused;
            let _ = h.update(move |t: &mut SimpleClipTray| t.paused = paused);
        }
    }
}
