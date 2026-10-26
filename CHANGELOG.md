# Changelog

Versions: a big update raises the first number, a minor update the middle one, and a bug fix the last one.

## 1.0.0

SimpleClip for Linux: the Windows app (5.0.2) ported to Wayland and PipeWire, with the
same look and the same features.

### Added
- Always-on replay buffer: the screen through the ScreenCast portal and PipeWire, encoded on the GPU (VA-API, then NVENC, then x264) and kept in RAM. The screen is chosen once and remembered.
- Desktop sound and an optional microphone, mixed into one track or kept on separate tracks, with volume sliders from 0 to 200%.
- Shortcuts (`Alt+F10` clip, `Alt+F9` screenshot, `Alt+F8` open the window) registered with the compositor: managed binds on Hyprland, the GlobalShortcuts portal on KDE and GNOME. Any key can be used.
- The save card (detailed or minimal, any corner, with or without the slide) as a layer-shell overlay, plus the save sound.
- The app: setup guide, home, clip gallery with grouping, bookmarks, folders, notes and search, trim with a live player, copy file, compress for Discord, presets, ten themes, and updates from inside the app.
- Tray icon, start at login, and the `sc` command line.
- Discord status that shows only games (Steam including Proton, Heroic, Lutris, and a list of well-known titles).
- `install.sh` and `uninstall.sh`, no root needed.
