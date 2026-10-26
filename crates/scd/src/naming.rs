use sc_core::config::{Config, FolderPolicy};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Resolve the full output path for a saved clip from config: base directory,
/// folder policy (flat / per-day / per-app), and the filename template.
pub fn clip_path(cfg: &Config, app: Option<&str>) -> PathBuf {
    media_path(cfg, app, "mp4")
}

/// Same naming/folder rules as clips, for a screenshot (`png`).
pub fn screenshot_path(cfg: &Config, app: Option<&str>) -> PathBuf {
    media_path(cfg, app, "png")
}

fn media_path(cfg: &Config, app: Option<&str>, ext: &str) -> PathBuf {
    let base = cfg.save.resolve_dir();

    let (y, mo, d, h, mi, s) = now();
    let dir = match cfg.save.folder_policy {
        FolderPolicy::Flat => base,
        FolderPolicy::PerDay => base.join(format!("{y:04}-{mo:02}-{d:02}")),
        FolderPolicy::PerApp => base.join(app.unwrap_or("unknown")),
    };

    let time = format!("{h:02}{mi:02}{s:02}");
    let name = cfg
        .save
        .filename_template
        .replace("{date}", &format!("{y:04}{mo:02}{d:02}"))
        .replace("{time}", &time)
        .replace("{app}", app.unwrap_or(""));
    let name = clean(&name);
    let path = dir.join(format!("{name}.{ext}"));
    // Same-day clips without a time in the name would collide; add one if needed,
    // and a counter if even that is taken (saves in the same second must never
    // overwrite each other).
    if !path.exists() {
        return path;
    }
    let timed = path.with_file_name(format!("{name}-{time}.{ext}"));
    if !timed.exists() {
        return timed;
    }
    (2u32..)
        .map(|n| path.with_file_name(format!("{name}-{time}-{n}.{ext}")))
        .find(|p| !p.exists())
        .unwrap_or(timed)
}

/// Collapse anything that isn't alphanumeric or `_` into single dashes, so an
/// empty {app} in "sc-{app}-{date}" yields "sc-<date>", not "sc--<date>".
fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.chars() {
        let c = if ch.is_ascii_alphanumeric() || ch == '_' {
            ch
        } else {
            '-'
        };
        if c == '-' {
            if prev_dash {
                continue;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
        out.push(c);
    }
    out.trim_matches(&['-', '_'][..]).to_string()
}

/// Best-effort foreground app/game name for the {app} token: the game's name
/// when it is one, else the program's window class.
pub fn foreground_app() -> Option<String> {
    let fg = crate::lin::foreground()?;
    // The save cards and our own windows aren't a meaningful clip source.
    if matches!(fg.class.as_str(), "simpleclip" | "scd" | "sc-gui") {
        return None;
    }
    let name = crate::games::detect(&fg).unwrap_or(fg.class);
    let app = clean(&name.to_ascii_lowercase());
    (!app.is_empty()).then_some(app)
}

/// Local wall-clock time, so clip names match the user's clock.
fn now() -> (i64, u32, u32, u32, u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return now_utc();
    }
    (
        tm.tm_year as i64 + 1900,
        tm.tm_mon as u32 + 1,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
        tm.tm_sec as u32,
    )
}

fn now_utc() -> (i64, u32, u32, u32, u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, mo, d) = civil_from_days(days);
    (
        y,
        mo,
        d,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Howard Hinnant's days-to-civil-date algorithm (days since 1970-01-01 -> Y/M/D).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_second_saves_get_distinct_names() {
        let dir = std::env::temp_dir().join(format!("sc-naming-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut cfg = Config::default();
        cfg.save.directory = Some(dir.clone());
        cfg.save.folder_policy = FolderPolicy::Flat;
        std::fs::create_dir_all(&dir).unwrap();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..6 {
            let p = media_path(&cfg, Some("game"), "mp4");
            assert!(seen.insert(p.clone()), "reused {}", p.display());
            std::fs::write(&p, b"x").unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
