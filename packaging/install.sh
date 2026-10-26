#!/usr/bin/env bash
# Build SimpleClip and install it for the current user. No root needed.
#
#   ./packaging/install.sh            build from source and install
#   ./packaging/install.sh --no-build install the binaries already in target/release
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS="$DATA/applications"
ICONS="$DATA/icons/hicolor"
SHARE="$DATA/simpleclip"

if [[ "${1:-}" != "--no-build" ]]; then
    echo "==> Building release binaries"
    cargo build --release --manifest-path "$ROOT/Cargo.toml"
fi

# Stop a running daemon first so the binary can be replaced.
if [[ -x "$BIN/sc" ]]; then "$BIN/sc" quit >/dev/null 2>&1 || true; sleep 0.5; fi

mkdir -p "$BIN" "$APPS" "$ICONS/256x256/apps" "$ICONS/scalable/apps" "$SHARE"
for b in scd sc sc-gui; do
    install -m 0755 "$ROOT/target/release/$b" "$BIN/$b.new"
    mv -f "$BIN/$b.new" "$BIN/$b"
    echo "    installed $BIN/$b"
done

sed "s|@BIN@|$BIN|g" "$ROOT/packaging/simpleclip.desktop" > "$APPS/simpleclip.desktop"
install -m 0644 "$ROOT/assets/icon-256.png" "$ICONS/256x256/apps/simpleclip.png"
install -m 0644 "$ROOT/assets/icon.svg" "$ICONS/scalable/apps/simpleclip.svg"
install -m 0755 "$ROOT/packaging/uninstall.sh" "$SHARE/uninstall.sh"
echo "    installed the launcher, icon and uninstaller"

command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$ICONS" 2>/dev/null || true

case ":$PATH:" in
    *":$BIN:"*) ;;
    *) echo "    note: $BIN is not on your PATH" ;;
esac

cat <<EOF

==> Done.

Open SimpleClip from your app launcher, or run: sc-gui

The first launch starts the daemon (scd) and your desktop asks which screen to
share, once. Shortcuts (Alt+F10 saves a clip) are set up for you on Hyprland,
KDE and GNOME. On another compositor, bind a key to: $BIN/sc save
EOF
