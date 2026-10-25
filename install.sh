#!/bin/sh
# Build panini-oled, put the binary in bin/, and run it as a systemd user service.
# Re-run to update after pulling or editing the code.
set -eu

root=$(cd "$(dirname "$0")" && pwd)
bus=/dev/i2c-7
unit_dir=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user
unit=panini-oled.service

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found in PATH, install Rust from https://rustup.rs" >&2
    exit 1
fi
if [ ! -e "$bus" ]; then
    echo "error: $bus not found. Is this the Orin Nano in the Panini case?" >&2
    exit 1
fi
if [ ! -r "$bus" ] || [ ! -w "$bus" ]; then
    echo "error: no access to $bus. Run 'sudo usermod -aG i2c $USER', log in again, then re-run" >&2
    exit 1
fi

echo "==> building" >&2
cargo build --release --locked --manifest-path "$root/Cargo.toml"

# Copy then rename, so a running service keeps its old binary until it is restarted.
mkdir -p "$root/bin"
cp "$root/target/release/panini-oled" "$root/bin/panini-oled.new"
mv "$root/bin/panini-oled.new" "$root/bin/panini-oled"

echo "==> installing $unit_dir/$unit" >&2
mkdir -p "$unit_dir"
sed "s|@BIN@|$root/bin/panini-oled|" "$root/systemd/$unit.in" >"$unit_dir/$unit"
systemctl --user daemon-reload
systemctl --user enable "$unit"
systemctl --user restart "$unit"

# Without lingering, user services only run while you are logged in.
if [ "$(loginctl show-user "$USER" -p Linger --value 2>/dev/null)" != "yes" ]; then
    echo "==> enabling linger so the service starts at boot" >&2
    if ! loginctl enable-linger "$USER"; then
        echo "warning: could not enable linger, run 'sudo loginctl enable-linger $USER'" >&2
    fi
fi

systemctl --user --no-pager status "$unit" | head -n 3 >&2
echo "installed: $root/bin/panini-oled" >&2
