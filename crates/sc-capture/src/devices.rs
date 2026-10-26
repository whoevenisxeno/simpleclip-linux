//! What can be captured. The screen itself is picked in the portal's dialog, so
//! there is nothing to enumerate for video; audio devices come from PipeWire's
//! PulseAudio-compatible `pactl`.

use crate::{Error, Result};
use sc_core::audio::AudioDevice;
use sc_core::capture::MonitorInfo;
use std::process::Command;

/// Displays are chosen in the system's share dialog (and remembered), so the
/// app has nothing to list. Kept for parity with the settings screens.
pub fn list_monitors() -> Result<Vec<MonitorInfo>> {
    Ok(Vec::new())
}

fn pactl(kind: &str) -> Result<Vec<serde_json::Value>> {
    let out = Command::new("pactl")
        .args(["--format=json", "list", kind])
        .output()
        .map_err(|e| Error::Other(format!("pactl: {e}")))?;
    if !out.status.success() {
        return Err(Error::Other("pactl failed".into()));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| Error::Other(format!("pactl output: {e}")))
}

/// Outputs that can be tapped for desktop audio (`is_monitor: true`, ids for
/// `Options::audio_device`) and microphones (`is_monitor: false`, ids for
/// `Options::mic_device`). Ids are PipeWire node names.
pub fn list_audio_devices() -> Result<Vec<AudioDevice>> {
    let mut out = Vec::new();
    for (kind, is_monitor) in [("sinks", true), ("sources", false)] {
        for d in pactl(kind)? {
            let id = d["name"].as_str().unwrap_or_default().to_string();
            // A source that is a sink's monitor is not a microphone.
            if id.is_empty() || (!is_monitor && id.ends_with(".monitor")) {
                continue;
            }
            let name = d["description"].as_str().unwrap_or(&id).to_string();
            out.push(AudioDevice {
                id,
                name,
                is_monitor,
            });
        }
    }
    Ok(out)
}
