# Open questions

| # | Question | Status |
|---|----------|--------|
| Q1 | Zero-copy capture: take the DMA-BUF the compositor offers and hand it to VA-API directly, instead of copying through system memory. | Open. CPU readback costs about 1 ms a frame at 1080p60 on the reference machine. |
| Q2 | HEVC and AV1 (the config has the field, only H.264 is wired up). | Open. |
| Q3 | Choosing the screen inside the app instead of the portal's dialog. | Open. Needs per-compositor output capture instead of the portal. |
| Q4 | Packaging: AUR, AppImage, Flatpak (a Flatpak needs the portal for global shortcuts and PipeWire, both available). | Open. |
| Q5 | Foreground game detection on GNOME and KDE (only Hyprland and sway can be asked what is in front). | Open. |
