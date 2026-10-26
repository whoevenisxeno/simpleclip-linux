# Decisions

## D-0001 · This is a port of the Windows app, not a new design

The Windows app (SimpleClip for Windows 5.0.2) is the reference for how the app looks
and behaves. The shared crates (`sc-core`, `sc-ffmpeg`, `sc-gui`, most of `scd`) are
the same code. What is Linux-specific is replaced one for one:

| Windows | Linux |
|---------|-------|
| Windows Graphics Capture | ScreenCast portal + PipeWire |
| WASAPI loopback and mic | PipeWire sink monitor and source streams |
| NVENC / AMF / QSV / x264 / Media Foundation | VA-API, then NVENC, then x264 |
| `RegisterHotKey` | Hyprland binds, or the GlobalShortcuts portal |
| Win32 tray | StatusNotifierItem (`ksni`) |
| GDI layered window for the save card | `wlr-layer-shell` surface drawn with tiny-skia |
| Registry autostart | XDG autostart entry (and `exec-once` on Hyprland) |
| Segoe UI, Segoe Fluent Icons | Inter and Phosphor, bundled (both open licences) |
| Inno Setup installer, robocopy updater | `install.sh`, and a tarball swapped in place |
| Discord named pipe | Discord's `discord-ipc-N` Unix socket |
| Recycle Bin, clipboard file list | freedesktop trash, `text/uri-list` on the clipboard |

## D-0002 · Fonts and icons are bundled

The Windows app takes Segoe UI and Segoe Fluent Icons from the system. Neither exists
on Linux, and relying on whatever sans-serif is installed would change every width
in the layout. Inter (OFL) and Phosphor (MIT) are embedded, so the app is laid out
the same on every machine.

## D-0003 · Wayland first; shortcuts are the compositor's job

There is no portable way for an app to grab a key on Wayland. Hyprland gets managed
binds (the app writes `~/.config/hypr/simpleclip.conf` and sources it once), KDE and
GNOME get the GlobalShortcuts portal, and anything else documents the three `sc`
commands to bind. A shortcut the compositor already owns can not be recorded in the
app's own recorder, because the compositor never passes that key on.

## D-0004 · The share dialog is asked once

`PersistMode::ExplicitlyRevoked` plus the restore token (stored in
`~/.local/share/simpleclip/screencast-token`). Settings has a "Choose..." button that
forgets the token and asks again. There is no in-app display picker, because the
portal owns that choice.

## D-0005 · The shared clock starts when the first frame is ready

The share dialog can stay open for any length of time, and the encoder times its
packets from the first frame it receives. The capture epoch is therefore taken after
the first frame arrives, not when `start` is called; otherwise audio would sit
minutes ahead of video.

## D-0006 · FFmpeg is linked from the system

`ffmpeg-next` 9.0 against the distro's FFmpeg 9. Its enum matches break on a
mismatched major version, so the crate version follows the FFmpeg the distro ships.
