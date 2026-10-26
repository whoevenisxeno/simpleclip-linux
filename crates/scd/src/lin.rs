//! Desktop glue for Linux: single instance, what is in front, autostart, the
//! save sound, and bringing the app window up.

use sc_core::config::GeneralSettings;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The runtime directory (`$XDG_RUNTIME_DIR/simpleclip`), made on demand.
pub fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("simpleclip");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Take the daemon's lock. Returns false when another daemon already holds it.
/// The lock lives as long as the process.
pub fn acquire_single_instance() -> bool {
    use std::os::fd::AsRawFd;
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(runtime_dir().join("scd.lock"))
    else {
        return true; // can't tell; let the IPC socket decide
    };
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if locked {
        std::mem::forget(file); // held until exit
    }
    locked
}

/// The program in front: its window class, its executable when it can be
/// found, and whether it fills the screen.
#[derive(Clone, Debug, Default)]
pub struct Foreground {
    pub class: String,
    pub exe: Option<PathBuf>,
    pub fullscreen: bool,
}

fn exe_of(pid: i64) -> Option<PathBuf> {
    (pid > 0)
        .then(|| std::fs::read_link(format!("/proc/{pid}/exe")).ok())
        .flatten()
}

fn run_json(cmd: &str, args: &[&str]) -> Option<serde_json::Value> {
    let out = Command::new(cmd)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| serde_json::from_slice(&out.stdout).ok())
        .flatten()
}

/// What is in front right now. Wayland has no portable way to ask, so this
/// speaks Hyprland and sway; elsewhere it is `None` and clips are simply not
/// named after a game.
pub fn foreground() -> Option<Foreground> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        let v = run_json("hyprctl", &["activewindow", "-j"])?;
        let class = v["class"].as_str().filter(|c| !c.is_empty())?.to_string();
        return Some(Foreground {
            class,
            exe: exe_of(v["pid"].as_i64().unwrap_or(0)),
            // 0 = tiled/floating, 1 = maximized, 2 = fullscreen.
            fullscreen: v["fullscreen"].as_i64().unwrap_or(0) >= 2,
        });
    }
    if std::env::var_os("SWAYSOCK").is_some() {
        fn focused(n: &serde_json::Value) -> Option<&serde_json::Value> {
            if n["focused"].as_bool() == Some(true) && n["pid"].as_i64().is_some() {
                return Some(n);
            }
            ["nodes", "floating_nodes"]
                .iter()
                .filter_map(|k| n[*k].as_array())
                .flatten()
                .find_map(focused)
        }
        let tree = run_json("swaymsg", &["-t", "get_tree"])?;
        let n = focused(&tree)?;
        let class = n["app_id"]
            .as_str()
            .or_else(|| n["window_properties"]["class"].as_str())
            .filter(|c| !c.is_empty())?
            .to_string();
        return Some(Foreground {
            class,
            exe: exe_of(n["pid"].as_i64().unwrap_or(0)),
            fullscreen: n["fullscreen_mode"].as_i64().unwrap_or(0) > 0,
        });
    }
    None
}

// ------------------------------------------------------------ the sound --

static SAVE_WAV: &[u8] = include_bytes!("../../../assets/sounds/save.wav");

/// The built-in save sound as a file players can open.
fn builtin_sound() -> Option<PathBuf> {
    let dir = directories::ProjectDirs::from("", "", "SimpleClip")?
        .cache_dir()
        .to_path_buf();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("save.wav");
    if std::fs::metadata(&path).map(|m| m.len()).ok() != Some(SAVE_WAV.len() as u64) {
        std::fs::write(&path, SAVE_WAV).ok()?;
    }
    Some(path)
}

/// Play a sound file with whatever player is installed (PipeWire first).
pub fn play_file(path: &Path) {
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        for (cmd, args) in [
            ("pw-play", &[][..]),
            ("paplay", &[][..]),
            ("aplay", &["-q"][..]),
            (
                "ffplay",
                &["-nodisp", "-autoexit", "-loglevel", "quiet"][..],
            ),
        ] {
            let ran = Command::new(cmd)
                .args(args)
                .arg(&path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if ran.is_ok_and(|s| s.success()) {
                return;
            }
        }
    });
}

/// Play the save sound: the user's own file if one is set and exists, else the
/// built-in one. Async; returns immediately.
pub fn play_save_sound(g: &GeneralSettings) {
    if let Some(file) = g.save_sound_file.as_ref().filter(|f| f.exists()) {
        play_file(file);
    } else if let Some(file) = builtin_sound() {
        play_file(&file);
    }
}

// ------------------------------------------------------------ autostart --

fn autostart_file() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.config_dir().join("autostart/simpleclip.desktop"))
}

pub fn autostart_enabled() -> bool {
    autostart_file().is_some_and(|p| p.exists())
}

/// Keep the XDG autostart entry in step with the setting.
pub fn set_autostart(on: bool) {
    let Some(path) = autostart_file() else { return };
    if !on {
        let _ = std::fs::remove_file(path);
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=SimpleClip\n\
         Comment=Instant replay: saves the last seconds of your screen\n\
         Exec={} --foreground\nIcon=simpleclip\nTerminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    if std::fs::read_to_string(&path).ok().as_deref() == Some(entry.as_str()) {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, entry);
}

// ----------------------------------------------------------- the window --

/// A program installed next to this one (or on PATH).
pub fn sibling(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// The app window's Wayland app id / X11 class.
const GUI_CLASS: &str = "simpleclip";

/// Bring the SimpleClip window up (a new window if none is open). Returns
/// false if it could not be started.
pub fn open_gui() -> bool {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        let class_re = format!("class:^({GUI_CLASS})$");
        if let Some(clients) = run_json("hyprctl", &["clients", "-j"]) {
            let ours = clients
                .as_array()
                .and_then(|a| a.iter().find(|c| c["class"].as_str() == Some(GUI_CLASS)));
            if let Some(win) = ours {
                let focused = run_json("hyprctl", &["activewindow", "-j"])
                    .is_some_and(|a| a["class"].as_str() == Some(GUI_CLASS));
                let on_special = win["workspace"]["name"]
                    .as_str()
                    .is_some_and(|n| n.starts_with("special:"));
                // Already in front: pressing the shortcut again tucks it away.
                if focused && !on_special {
                    let _ = Command::new("hyprctl")
                        .args([
                            "dispatch",
                            "movetoworkspacesilent",
                            &format!("special:simpleclip,{class_re}"),
                        ])
                        .status();
                } else if on_special {
                    let ws = run_json("hyprctl", &["activeworkspace", "-j"])
                        .and_then(|w| w["id"].as_i64())
                        .unwrap_or(1);
                    let _ = Command::new("hyprctl")
                        .args([
                            "dispatch",
                            "movetoworkspacesilent",
                            &format!("{ws},{class_re}"),
                        ])
                        .status();
                    let _ = Command::new("hyprctl")
                        .args(["dispatch", "focuswindow", &class_re])
                        .status();
                } else {
                    let _ = Command::new("hyprctl")
                        .args(["dispatch", "focuswindow", &class_re])
                        .status();
                }
                return true;
            }
        }
    }
    Command::new(sibling("sc-gui"))
        .stdin(Stdio::null())
        .spawn()
        .is_ok()
}

/// Open a folder in the file manager.
pub fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    let _ = Command::new("xdg-open")
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Show a short card (errors, hotkey problems).
pub fn notify(title: &str, body: &str) {
    let kind = if title.to_ascii_lowercase().contains("couldn")
        || body.to_ascii_lowercase().contains("couldn")
    {
        crate::popup::Kind::Error
    } else {
        crate::popup::Kind::Info
    };
    crate::popup::show(crate::popup::Popup::simple(kind, title, body));
}
