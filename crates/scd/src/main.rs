mod actions;
mod audio_mix;
mod discord;
mod games;
mod hotkeys;
mod lin;
mod naming;
mod notify;
mod pipeline;
mod popup;
mod reload;
mod server;
mod state;
mod thumbs;
mod tray;

use anyhow::{Context, Result};
use clap::Parser;
use sc_core::config::Config;
use state::Daemon;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "scd",
    about = "SimpleClip daemon: owns capture, buffer, encode, save"
)]
struct Args {
    /// Run in the foreground (do not detach); logs also go to stderr.
    #[arg(long)]
    foreground: bool,
    /// Verbose (debug) logging.
    #[arg(long, short)]
    verbose: bool,
    /// Config file path (defaults to the platform config dir).
    #[arg(long)]
    config: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let _log_guard = init_logging(args.verbose, args.foreground);

    if !lin::acquire_single_instance() {
        tracing::warn!("scd is already running; exiting");
        return Ok(());
    }

    let config_path = match args.config {
        Some(p) => p,
        None => Config::default_path().context("resolving config path")?,
    };
    let config = Config::load_or_recover(&config_path);

    popup::configure(&config.general);
    let daemon = Daemon::new(config);
    reload::spawn_watcher(config_path, daemon.clone());
    // Tidy the preview cache in the background; it must never slow start-up.
    std::thread::spawn(thumbs::prune);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "scd starting");

    // Start capture off-thread so the IPC server is reachable immediately (the
    // portal share dialog can take a while to be answered the first time).
    {
        let d = daemon.clone();
        std::thread::spawn(move || {
            // A failure is usually transient (no display yet at login, the
            // portal still starting), so keep trying instead of giving up.
            let mut wait = 5;
            while !d.start_capture() {
                std::thread::sleep(std::time::Duration::from_secs(wait));
                wait = (wait * 2).min(60);
            }
        });
    }

    // Keep the XDG autostart entry in step with the setting (on by default), so
    // the replay buffer is always running in the background after login.
    lin::set_autostart(daemon.config.read().unwrap().general.autostart);
    hotkeys::spawn(daemon.clone());
    discord::spawn(daemon.clone());
    let server_daemon = daemon.clone();
    std::thread::Builder::new()
        .name("sc-ipc".into())
        .spawn(move || {
            if let Err(e) = server::serve(server_daemon) {
                tracing::error!(error = format!("{e:#}"), "IPC server failed");
                std::process::exit(1);
            }
        })
        .context("ipc thread")?;
    announce_startup(daemon.clone());
    // The tray owns the main thread until the user quits.
    tray::run(daemon);
    tracing::info!("scd quitting");
    Ok(())
}

/// A card shortly after start-up: SimpleClip is running in the background, and
/// whether a newer version is out. Waits a little for the network at login.
fn announce_startup(daemon: Daemon) {
    std::thread::Builder::new()
        .name("sc-startup-card".into())
        .spawn(move || {
            let (notify_on, check, hotkey, secs) = {
                let c = daemon.config.read().unwrap();
                (
                    c.general.notify,
                    c.general.update_check,
                    c.hotkeys.save.clone(),
                    c.buffer.replay_duration_secs,
                )
            };
            if !notify_on {
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
            let mut lines = vec![format!("{hotkey} saves the last {secs}s")];
            if check {
                let current = env!("CARGO_PKG_VERSION");
                let mut result = Err(String::new());
                for attempt in 0..4 {
                    result = sc_core::update::fetch_latest(current);
                    if result.is_ok() {
                        break;
                    }
                    if attempt < 3 {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                    }
                }
                lines.push(match result {
                    Ok(Some(r)) => format!("Update {} is available. Open SimpleClip.", r.version),
                    Ok(None) => format!("You are up to date ({current})"),
                    Err(_) => "Couldn't check for updates".to_string(),
                });
            }
            popup::show(popup::Popup {
                kind: popup::Kind::Info,
                compact: false,
                title: "SimpleClip is running".into(),
                lines,
            });
        })
        .ok();
}

/// Logging goes to stderr (journald when run as a service) and, so a session
/// started from autostart leaves a trace, to a rolling file under
/// `~/.local/share/SimpleClip/logs`. The returned guard must outlive `main` so
/// buffered lines are flushed.
fn init_logging(
    verbose: bool,
    foreground: bool,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{fmt, EnvFilter};
    let level = if verbose { "debug" } else { "info" };
    let filter = EnvFilter::try_from_env("SC_LOG").unwrap_or_else(|_| EnvFilter::new(level));

    let dir = directories::ProjectDirs::from("", "", "SimpleClip")
        .map(|d| d.data_local_dir().join("logs"))
        .unwrap_or_else(|| PathBuf::from("."));
    prune_logs(&dir, 7);
    let appender = tracing_appender::rolling::daily(dir, "scd.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let file = fmt::layer()
        .with_target(false)
        .with_ansi(false)
        .with_writer(writer);
    // Without --foreground there may be no terminal at all; log to the file only.
    let stderr = foreground.then(|| fmt::layer().with_target(false));
    tracing_subscriber::registry()
        .with(filter)
        .with(file)
        .with(stderr)
        .init();

    // A background process otherwise dies silently; leave a trace in the log.
    std::panic::set_hook(Box::new(|info| {
        tracing::error!("panic: {info}");
    }));
    Some(guard)
}

/// Keep only the newest `keep` daily log files so logs never pile up.
fn prune_logs(dir: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("scd.log"))
        })
        .collect();
    // Names end in the date, so sorting by name sorts by age.
    logs.sort();
    let drop = logs.len().saturating_sub(keep);
    for p in logs.into_iter().take(drop) {
        let _ = std::fs::remove_file(p);
    }
}
