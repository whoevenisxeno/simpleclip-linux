//! The things a shortcut, a tray entry or the CLI can ask the daemon to do.

use crate::state::Daemon;

/// Run the action with this id (`save`, `screenshot` or `open`).
pub fn run(daemon: &Daemon, id: &str) {
    match id {
        "save" => save(daemon),
        "screenshot" => screenshot(daemon),
        "open" if !crate::lin::open_gui() => crate::lin::notify(
            "SimpleClip",
            "Couldn't find the SimpleClip window program (sc-gui).",
        ),
        _ => {}
    }
}

pub fn save(daemon: &Daemon) {
    if let Err(e) = daemon.save_clip(None) {
        tracing::warn!(error = %e, "save failed");
        crate::lin::notify("SimpleClip", &format!("Couldn't save clip: {e}"));
    }
}

pub fn screenshot(daemon: &Daemon) {
    if let Err(e) = daemon.screenshot() {
        tracing::warn!(error = %e, "screenshot failed");
        crate::lin::notify("SimpleClip", &format!("Couldn't take screenshot: {e}"));
    }
}
