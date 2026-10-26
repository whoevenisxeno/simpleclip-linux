#!/usr/bin/env bash
# Remove SimpleClip for the current user. Clips in your Videos folder are left
# alone (the app offers to move them to the trash before it runs this).
#
#   uninstall.sh          ask first
#   uninstall.sh --yes    do not ask
set -uo pipefail

BIN="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}"

if [[ "${1:-}" != "--yes" ]]; then
    read -r -p "Remove SimpleClip and its settings? [y/N] " ans
    [[ "$ans" == [yY]* ]] || exit 0
fi

"$BIN/sc" quit >/dev/null 2>&1 || true
sleep 0.5
pkill -x scd >/dev/null 2>&1 || true

rm -f "$BIN/scd" "$BIN/sc" "$BIN/sc-gui"
rm -f "$DATA/applications/simpleclip.desktop"
rm -f "$DATA/icons/hicolor/256x256/apps/simpleclip.png" "$DATA/icons/hicolor/scalable/apps/simpleclip.svg"
rm -f "$CONF/autostart/simpleclip.desktop" "$CONF/systemd/user/simpleclip.service"

# The Hyprland snippet and the line that sources it.
rm -f "$CONF/hypr/simpleclip.conf"
if [[ -f "$CONF/hypr/hyprland.conf" ]]; then
    sed -i '/# SimpleClip shortcuts (managed)/d; /simpleclip\.conf/d' "$CONF/hypr/hyprland.conf"
    command -v hyprctl >/dev/null && hyprctl reload >/dev/null 2>&1 || true
fi

rm -rf "$CONF/simpleclip" "$DATA/simpleclip"
echo "SimpleClip removed."
