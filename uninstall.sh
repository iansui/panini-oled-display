#!/bin/sh
# Stop and remove the panini-oled user service and the installed binary.
# The source tree and target/ are left alone; linger is left as it is.
set -eu

root=$(cd "$(dirname "$0")" && pwd)
unit_dir=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user
unit=panini-oled.service

# Stopping the service blanks the panel.
systemctl --user disable --now "$unit" 2>/dev/null || true
rm -f "$unit_dir/$unit"
systemctl --user daemon-reload
rm -rf "$root/bin"

echo "removed $unit and $root/bin" >&2
echo "linger is unchanged; turn it off with 'loginctl disable-linger $USER' if nothing else needs it" >&2
