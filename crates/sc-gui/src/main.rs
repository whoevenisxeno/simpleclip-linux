mod app;
mod config_io;
mod daemon;
mod dialogs;
mod library;
mod player;
mod presets;
mod screens;
mod star;
mod theme;
mod themes;
mod thumbs;
mod update;
mod widgets;

use app::ScApp;

/// The app icon, decoded for the window.
fn window_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../../assets/icon-256.png");
    let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes[..]))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some(egui::IconData {
        rgba: buf,
        width: info.width,
        height: info.height,
    })
}

/// Only one SimpleClip window at a time: the first instance keeps an exclusive
/// lock on a file for as long as it runs, and a second one exits straight away.
/// Returns `false` when another window already holds the lock.
fn claim_single_instance() -> (bool, Option<std::fs::File>) {
    use std::os::fd::AsRawFd;
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("simpleclip");
    let _ = std::fs::create_dir_all(&dir);
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("gui.lock"))
    else {
        return (true, None);
    };
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        (true, Some(file))
    } else {
        (false, None)
    }
}

fn main() -> eframe::Result<()> {
    // (The design-review run opens its own windows and must not be blocked.)
    let _instance = if std::env::var_os("SC_GUI_SHOTS").is_some() {
        None
    } else {
        let (first, lock) = claim_single_instance();
        if !first {
            return Ok(());
        }
        lock
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1080.0, 720.0])
        .with_min_inner_size([820.0, 560.0])
        .with_title("SimpleClip")
        // The compositor matches this to the desktop entry (icon, taskbar grouping)
        // and the daemon finds the window by it.
        .with_app_id("simpleclip");
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "SimpleClip",
        options,
        Box::new(|cc| Ok(Box::new(ScApp::new(cc)))),
    )
}
