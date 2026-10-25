# panini-oled

A small Rust service for the 128x64 OLED of the Roboco Panini case for the
Jetson Orin Nano. It shows two pages that alternate every 10 seconds: the
hostname, IP address and disk use, then the CPU temperature, CPU use and RAM
use.

```text
                  1/2                     2/2
Host: my-jetson         CPU Temp: 53.6°C
WiFi 192.0.2.10         CPU Use: 05.20%
Disk: 12G/233G 6%       RAM Use: 41.30%
```

## Hardware

| Item    | Value                              |
| ------- | ---------------------------------- |
| Board   | Jetson Orin Nano                   |
| Panel   | SSD1306, 128x64, I2C               |
| Bus     | `/dev/i2c-7` (header pins 3 and 5) |
| Address | `0x3c`                             |

Check the wiring with `i2cdetect` (from `i2c-tools`). The panel shows up as
`3c`:

```bash
i2cdetect -y -r 7
# 30: -- -- -- -- -- -- -- -- -- -- -- -- 3c -- -- --
```

## Requirements

| Dependency   | Needed for          | Install                      |
| ------------ | ------------------- | ---------------------------- |
| Rust 1.99.1+ | building            | <https://rustup.rs>          |
| `i2c` group  | access without root | `sudo usermod -aG i2c $USER` |
| `i2c-tools`  | wiring check only   | `sudo apt install i2c-tools` |

## Install

```bash
./install.sh
```

The script does four things:

1. It runs `cargo build --release --locked` and copies the binary to
   `bin/panini-oled`.
2. It writes `~/.config/systemd/user/panini-oled.service` from
   `systemd/panini-oled.service.in`.
3. It enables and restarts the service.
4. It runs `loginctl enable-linger $USER` so the service starts at boot
   without a login.

To update, pull or edit the code and run `./install.sh` again. It is safe to
run while the service is running.

## Usage

The service runs `bin/panini-oled` with the defaults. The binary can also be
run by hand:

```bash
bin/panini-oled --dry-run --once   # print page 1 to stdout, no panel
bin/panini-oled --dry-run --once --page 2
bin/panini-oled --once             # draw page 1, leave it, exit
bin/panini-oled --page-seconds 5   # flip pages every 5 seconds
bin/panini-oled --clear            # blank the panel
bin/panini-oled --bus /dev/i2c-1 --address 0x3d
```

| Flag             | Default       | Meaning                             |
| ---------------- | ------------- | ----------------------------------- |
| `--bus`          | `/dev/i2c-7`  | I2C bus device                      |
| `--address`      | `0x3c`        | 7-bit panel address, in hex         |
| `--interval`     | `1`           | seconds between reading updates     |
| `--page-seconds` | `10`          | seconds each page stays up          |
| `--page`         |               | show only page 1 or 2, no rotation  |
| `--disk`         | `/`           | mount point for the disk line       |
| `--thermal-zone` | `cpu-thermal` | thermal zone `type` to read         |
| `--once`         |               | draw one frame and exit             |
| `--dry-run`      |               | print frames to stdout, do not draw |
| `--clear`        |               | blank the panel and exit            |

Stop the service (`systemctl --user stop panini-oled`) before running the
binary.

## Service

```bash
systemctl --user status panini-oled
journalctl --user -u panini-oled -f   # logs
systemctl --user restart panini-oled
systemctl --user stop panini-oled     # blanks the panel
```

To change the service's flags, edit `ExecStart` in
`systemd/panini-oled.service.in` and run `./install.sh` again.

## Uninstall

```bash
./uninstall.sh
loginctl disable-linger $USER   # optional, if no other user service needs it
```

That stops the service, blanks the panel, and removes the unit file and
`bin/`.  
