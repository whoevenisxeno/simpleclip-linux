//! Global shortcuts, registered with the compositor itself so nothing needs
//! elevated permissions and the keys work with any window in front.
//!
//! - Hyprland: a SimpleClip-owned config snippet with one bind per action, which
//!   calls the `sc` client, and an `exec-once` that starts the daemon at login.
//!   It is sourced once from the main config and Hyprland is reloaded only when
//!   something actually changed.
//! - Everywhere else with the GlobalShortcuts portal (KDE, GNOME 48+, COSMIC):
//!   the shortcuts are registered over D-Bus and activations come back as signals.
//!
//! Where neither exists, bind a key to `sc save`, `sc screenshot` and `sc open`
//! in the compositor's own settings.

use crate::state::Daemon;
use futures_util::StreamExt;
use sc_core::config::{Config, HotkeySettings};
use sc_core::keys::Shortcut;
use std::path::PathBuf;
use std::time::Duration;

/// The three things a shortcut can do: (config name, `sc` subcommand, label).
const ACTIONS: [(&str, &str, &str); 3] = [
    ("save", "save", "Save the last seconds as a clip"),
    ("screenshot", "screenshot", "Save a screenshot"),
    ("open", "open", "Open SimpleClip"),
];

fn spec_of<'a>(hk: &'a HotkeySettings, action: &str) -> &'a str {
    match action {
        "save" => &hk.save,
        "screenshot" => &hk.screenshot,
        _ => &hk.open,
    }
}

/// Start keeping the compositor's shortcuts in step with the config.
pub fn spawn(daemon: Daemon) {
    sync(&daemon.config.read().unwrap());
    if hypr_running() {
        return; // binds live in the compositor; nothing to listen for
    }
    std::thread::Builder::new()
        .name("sc-hotkeys".into())
        .spawn(move || {
            if let Err(e) = pollster::block_on(portal_loop(daemon)) {
                tracing::warn!(
                    error = %e,
                    "no global shortcut support here; bind keys to `sc save`, `sc screenshot` and `sc open` in your compositor"
                );
            }
        })
        .ok();
}

/// Bring the compositor's binds in line with the config (called again on every
/// config reload).
pub fn sync(cfg: &Config) {
    if hypr_running() {
        sync_hyprland(&cfg.hotkeys, cfg.general.autostart);
    }
}

fn hypr_running() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

// ----------------------------------------------------------- Hyprland --

fn hypr_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.config_dir().join("hypr"))
}

/// The `sc` client next to this program (falling back to PATH).
fn sc_binary() -> String {
    crate::lin::sibling("sc").display().to_string()
}

fn sync_hyprland(hk: &HotkeySettings, autostart: bool) {
    let sc = sc_binary();
    let mut content =
        String::from("# Managed by SimpleClip. Set the shortcuts in the app, not here.\n");
    if autostart {
        // A second daemon exits at once (it takes a lock), so this is safe even
        // though the config is sourced again on every reload.
        content.push_str(&format!(
            "exec-once = {}\n",
            crate::lin::sibling("scd").display()
        ));
    }
    for (name, cmd, _) in ACTIONS {
        let spec = spec_of(hk, name);
        if spec.trim().is_empty() {
            continue; // switched off
        }
        let Some(s) = Shortcut::parse(spec) else {
            tracing::warn!(
                shortcut = spec,
                what = name,
                "unrecognized shortcut; skipping"
            );
            crate::lin::notify(
                "SimpleClip",
                &format!("\"{spec}\" isn't a valid {name} shortcut. Fix it in Settings."),
            );
            continue;
        };
        let key = sc_core::keys::keysym(&s.key).unwrap_or(s.key.clone());
        content.push_str(&format!(
            "bind = {}, {key}, exec, {sc} {cmd}\n",
            s.hypr_mods()
        ));
    }

    let Some(conf) = hypr_dir().map(|d| d.join("simpleclip.conf")) else {
        return;
    };
    if std::fs::read_to_string(&conf).ok().as_deref() == Some(content.as_str()) {
        return; // already up to date; don't reload the compositor needlessly
    }
    if let Some(parent) = conf.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&conf, &content).is_err() {
        return;
    }
    ensure_sourced();
    let _ = std::process::Command::new("hyprctl").arg("reload").spawn();
    tracing::info!("Hyprland shortcuts synced");
}

fn ensure_sourced() {
    let Some(dir) = hypr_dir() else { return };
    let main = dir.join("hyprland.conf");
    let Ok(text) = std::fs::read_to_string(&main) else {
        return;
    };
    if text.contains("simpleclip.conf") {
        return;
    }
    let line = "\n# SimpleClip shortcuts (managed)\nsource = ~/.config/hypr/simpleclip.conf\n";
    let _ = std::fs::write(&main, format!("{text}{line}"));
}

// ------------------------------------------------------------- portal --

async fn portal_loop(daemon: Daemon) -> Result<(), ashpd::Error> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    let proxy = GlobalShortcuts::new().await?;
    let session = proxy.create_session(Default::default()).await?;
    let mut bound = daemon.config.read().unwrap().hotkeys.clone();
    let bind = |hk: &HotkeySettings| {
        ACTIONS
            .iter()
            .filter_map(|(name, _, label)| {
                let spec = spec_of(hk, name);
                if spec.trim().is_empty() {
                    return None;
                }
                let trigger = Shortcut::parse(spec).map(|s| s.portal_trigger());
                Some(NewShortcut::new(*name, *label).preferred_trigger(trigger.as_deref()))
            })
            .collect::<Vec<_>>()
    };
    proxy
        .bind_shortcuts(&session, &bind(&bound), None, Default::default())
        .await?;
    tracing::info!("global shortcuts registered through the portal");
    let mut activated = proxy.receive_activated().await?;
    loop {
        let tick = async_io::Timer::after(Duration::from_secs(1));
        futures_util::pin_mut!(tick);
        match futures_util::future::select(activated.next(), tick).await {
            futures_util::future::Either::Left((Some(a), _)) => {
                let d = daemon.clone();
                let id = a.shortcut_id().to_string();
                std::thread::spawn(move || crate::actions::run(&d, &id));
            }
            futures_util::future::Either::Left((None, _)) => return Ok(()),
            futures_util::future::Either::Right(_) => {
                let now = daemon.config.read().unwrap().hotkeys.clone();
                if now != bound {
                    bound = now;
                    proxy
                        .bind_shortcuts(&session, &bind(&bound), None, Default::default())
                        .await?;
                }
            }
        }
    }
}
