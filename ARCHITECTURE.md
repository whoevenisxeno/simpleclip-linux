# Architecture

SimpleClip is a long-lived **daemon** and several **thin clients**. The daemon is the
only component that touches capture, so a crash in the app or the command line can
not lose or corrupt the replay buffer.

```
                 ┌─────────────────────────────────────────────────────┐
  compositor     │ scd (daemon)                                        │
  shortcut ──▶ sc│                                                     │
                 │  portal + PipeWire ─▶ pacer ─▶ colour ─▶ VA-API/    │
  sc-gui ───────▶│  (screen)                       convert   NVENC/x264│
  (app)          │                                              │      │
  tray           │  PipeWire (desktop audio, mic) ─▶ mix ─▶ AAC │      │
                 │                                              ▼      │
                 │                          rolling packet rings (RAM) │
                 │  save: snapshot ─▶ MP4 muxer ─▶ file + save card    │
                 │  IPC server (versioned newline JSON)                │
                 └─────────────────────────────────────────────────────┘
```

## Crates

| Crate | Kind | Responsibility |
|-------|------|----------------|
| `sc-core` | lib | Shared contracts: config, IPC messages, the packet ring buffer, timestamps, shortcut names, the update check. No desktop code. |
| `sc-capture` | lib | The screen (xdg-desktop-portal ScreenCast + PipeWire), desktop audio and the microphone (PipeWire streams), the frame pacer, device listing. |
| `sc-ffmpeg` | lib | Encoders (VA-API, NVENC, libx264), AAC, the MP4 muxer, trim, compress, thumbnails and the preview player's decoder. FFmpeg through `ffmpeg-next`. |
| `scd` | bin | The daemon: pipeline, IPC server, shortcuts, tray, save cards, Discord status, game detection. |
| `sc` | bin | Command line client. One request, one reply. The compositor shortcuts call this. |
| `sc-gui` | bin | The app (egui): setup guide, gallery, trim, settings. Also just an IPC client. |

## The capture path

1. The daemon asks the **ScreenCast portal** for a screen (the desktop's own dialog,
   once; the **restore token** it returns is kept so later launches are silent).
2. **PipeWire** delivers frames only when the picture changes. A **pacer** thread
   re-emits the latest frame at the target rate, so a still desktop still produces
   regular frames and keyframes, and a 240 Hz screen is capped.
3. Frames go through BGRA to NV12 (`swscale`, BT.709), then to the encoder.
4. Desktop audio is a PipeWire capture of the default sink's monitor; the microphone
   is an ordinary capture stream. Both are asked for 48 kHz stereo float, stamped on
   the same clock as video, and passed through `audio_mix`, which pads gaps with
   silence so pauses and dropouts never slide sound against picture.

## The replay buffer

`PacketRing` (`sc-core/src/buffer.rs`) is a byte-capped ring of **encoded** packets,
not raw frames (a 1080p60 raw buffer would not fit in RAM). Eviction drops whole
leading GOPs, so the front is always a keyframe and a saved clip starts cleanly. One
ring for video and one per audio track.

Saving snapshots the last N seconds (clone under the lock, mux after), so capture
never pauses while a clip is written.

## Shortcuts, tray, cards

- **Shortcuts** are registered with the compositor: a managed config snippet on
  Hyprland (binds that run `sc save`, `sc screenshot`, `sc open`), the
  GlobalShortcuts portal elsewhere (KDE, GNOME, COSMIC).
- **Tray** is a StatusNotifierItem (`ksni`).
- **Save cards** are `wlr-layer-shell` surfaces drawn with `tiny-skia`: click-through,
  never focused, above full-screen windows. Without layer-shell they fall back to a
  desktop notification.

## IPC

A local socket (`interprocess`; an abstract Unix socket named `simpleclip.sock`).
Messages are newline-delimited JSON in an `Envelope { version, id, payload }`; the
version is checked on every read, and within a version only additive changes are
allowed. The schema is `sc-core/src/ipc.rs`. `SC_SOCKET` overrides the name for
tests and side-by-side daemons.

## Config

TOML in `~/.config/simpleclip/config.toml`. Hand-editable and hot-reloaded by a file
watcher. An invalid file is moved aside (`config.toml.bad`) and replaced with
defaults rather than crashing the daemon.
