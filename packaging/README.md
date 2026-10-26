# Packaging

## Local install (any distro)

```
./packaging/install.sh            # build from source, then install
./packaging/install.sh --no-build # install what is already in target/release
```

Installs `scd`, `sc` and `sc-gui` to `~/.local/bin`, a launcher and icon, and an
uninstaller at `~/.local/share/simpleclip/uninstall.sh`. No root.

## Starting the daemon at login

The daemon keeps this in step with the Settings toggle by itself: on Hyprland the managed
`~/.config/hypr/simpleclip.conf` gets an `exec-once`, elsewhere an XDG autostart entry is written to
`~/.config/autostart/simpleclip.desktop`. If your session runs neither, use the systemd unit:

```
mkdir -p ~/.config/systemd/user
cp packaging/systemd/simpleclip.service ~/.config/systemd/user/
systemctl --user enable --now simpleclip.service
```

The daemon needs the Wayland and portal environment, which a graphical-session user unit inherits.

## Shortcuts on a compositor without portal support

Bind these in the compositor's own settings:

```
sc save          # the last N seconds
sc screenshot
sc open          # the app window
```

## Release tarball

A tag `vX.Y.Z` builds `simpleclip-X.Y.Z-linux-x86_64.tar.gz` (the three programs) in an Arch
container and attaches it to a GitHub release, with the changelog entry as the notes. The in-app
updater downloads that file. It is linked against current Arch's FFmpeg.

## Planned

- AUR package
- AppImage / Flatpak
