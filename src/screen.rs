//! The pages of text and the SSD1306 panel they are drawn on. Each page is a marker in the
//! top-right corner and three rows below it.

use crate::error::Error;
use crate::net::Kind;
use crate::stats::DiskUsage;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_6X13;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use linux_embedded_hal::I2cdev;
use ssd1306::mode::BufferedGraphicsMode;
use ssd1306::prelude::*;
use ssd1306::{I2CDisplayInterface, Ssd1306};
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

pub const PAGES: usize = 2;
const LINE_HEIGHT: i32 = 16;
/// 128 px / 6 px per glyph.
const COLUMNS: usize = 21;

#[derive(Debug, Default)]
pub struct Snapshot {
    pub host: Option<String>,
    pub ip: Option<(Kind, Ipv4Addr)>,
    pub disk: Option<DiskUsage>,
    pub temp: Option<f32>,
    pub cpu: Option<f32>,
    pub mem: Option<f32>,
}

/// The three rows of page `n` (0-based): host, IP and disk; then temperature, CPU and RAM.
pub fn page(s: &Snapshot, n: usize) -> [String; 3] {
    let percent = |v: Option<f32>| v.map_or("N/A".into(), |v| format!("{v:05.2}%"));
    match n % PAGES {
        0 => [
            host_line(s.host.as_deref()),
            match s.ip {
                // Labels stay within 5 columns so the longest dotted quad still fits.
                Some((kind, ip)) => format!("{:<4} {ip}", kind.label()),
                None => "IP: Not found".into(),
            },
            disk_line(s.disk.as_ref()),
        ],
        _ => [
            format!(
                "CPU Temp: {}",
                s.temp.map_or("N/A".into(), |t| format!("{t:.1}°C"))
            ),
            format!("CPU Use: {}", percent(s.cpu)),
            format!("RAM Use: {}", percent(s.mem)),
        ],
    }
}

pub fn marker(n: usize) -> String {
    format!("{}/{PAGES}", n % PAGES + 1)
}

fn host_line(host: Option<&str>) -> String {
    const LABEL: &str = "Host: ";
    match host {
        Some(name) => format!(
            "{LABEL}{}",
            name.chars().take(COLUMNS - LABEL.len()).collect::<String>()
        ),
        None => format!("{LABEL}N/A"),
    }
}

fn disk_line(disk: Option<&DiskUsage>) -> String {
    match disk {
        Some(d) => format!(
            "Disk: {}/{} {}%",
            human_size(d.used),
            human_size(d.total),
            d.percent()
        ),
        None => "Disk: N/A".into(),
    }
}

/// Like `df -h`: powers of 1024, rounded up, one decimal below 10. Unlike df, 1000 and above
/// moves to the next unit, so a size is never wider than 4 characters.
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["B", "K", "M", "G", "T", "P", "E"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes}B");
    }
    let tenths = (v * 10.0).ceil();
    if tenths < 100.0 {
        return format!("{:.1}{}", tenths / 10.0, UNITS[unit]);
    }
    let whole = v.ceil();
    if whole >= 1000.0 && unit < UNITS.len() - 1 {
        return format!("1.0{}", UNITS[unit + 1]);
    }
    format!("{whole:.0}{}", UNITS[unit])
}

type Display =
    Ssd1306<I2CInterface<I2cdev>, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>;

pub struct Panel {
    display: Display,
    bus: PathBuf,
    addr: u8,
}

impl Panel {
    pub fn open(bus: &Path, addr: u8) -> Result<Self, Error> {
        check_bus(bus)?;
        let i2c = I2cdev::new(bus).map_err(|e| Error::BusOpen(bus.to_path_buf(), e.to_string()))?;
        let interface = I2CDisplayInterface::new_custom_address(i2c, addr);
        let display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();
        let mut panel = Panel {
            display,
            bus: bus.to_path_buf(),
            addr,
        };
        panel.display.init().map_err(|e| panel.error(e))?;
        Ok(panel)
    }

    pub fn draw(&mut self, marker: &str, lines: &[String; 3]) -> Result<(), Error> {
        self.display.clear_buffer();
        let style = MonoTextStyle::new(&FONT_6X13, BinaryColor::On);
        let top_right = TextStyleBuilder::new()
            .alignment(Alignment::Right)
            .baseline(Baseline::Top)
            .build();
        let right_edge = self.display.bounding_box().size.width as i32 - 1;
        Text::with_text_style(marker, Point::new(right_edge, 0), style, top_right)
            .draw(&mut self.display)
            .map_err(|e| self.error(e))?;
        for (row, line) in (1..).zip(lines) {
            Text::with_baseline(line, Point::new(0, row * LINE_HEIGHT), style, Baseline::Top)
                .draw(&mut self.display)
                .map_err(|e| self.error(e))?;
        }
        self.display.flush().map_err(|e| self.error(e))
    }

    pub fn clear(&mut self) -> Result<(), Error> {
        self.display.clear_buffer();
        self.display.flush().map_err(|e| self.error(e))
    }

    fn error(&self, e: impl std::fmt::Debug) -> Error {
        Error::Display {
            bus: self.bus.clone(),
            addr: self.addr,
            detail: format!("{e:?}"),
        }
    }
}

/// Open the bus once with std first, so a missing device or a missing i2c group gets its own
/// message instead of an opaque driver error.
fn check_bus(bus: &Path) -> Result<(), Error> {
    match OpenOptions::new().read(true).write(true).open(bus) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Err(Error::BusNotFound(bus.to_path_buf())),
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            Err(Error::BusPermission(bus.to_path_buf()))
        }
        Err(e) => Err(Error::BusOpen(bus.to_path_buf(), e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    fn full() -> Snapshot {
        Snapshot {
            host: Some("my-jetson".into()),
            ip: Some((Kind::Wifi, Ipv4Addr::new(192, 0, 2, 10))),
            disk: Some(DiskUsage {
                used: 11 * GIB + GIB / 2,
                avail: 210 * GIB,
                total: 233 * GIB,
            }),
            temp: Some(53.625),
            cpu: Some(5.2),
            mem: Some(41.3),
        }
    }

    #[test]
    fn first_page_shows_host_ip_and_disk() {
        assert_eq!(
            page(&full(), 0),
            ["Host: my-jetson", "WiFi 192.0.2.10", "Disk: 12G/233G 6%"]
        );
    }

    #[test]
    fn second_page_shows_temperature_cpu_and_ram() {
        assert_eq!(
            page(&full(), 1),
            ["CPU Temp: 53.6°C", "CPU Use: 05.20%", "RAM Use: 41.30%"]
        );
    }

    #[test]
    fn missing_readings_say_so() {
        let s = Snapshot::default();
        assert_eq!(page(&s, 0), ["Host: N/A", "IP: Not found", "Disk: N/A"]);
        assert_eq!(
            page(&s, 1),
            ["CPU Temp: N/A", "CPU Use: N/A", "RAM Use: N/A"]
        );
    }

    #[test]
    fn markers_count_the_pages() {
        assert_eq!(marker(0), "1/2");
        assert_eq!(marker(1), "2/2");
    }

    #[test]
    fn long_hostname_is_cut_to_fit() {
        let mut s = full();
        s.host = Some("a-very-long-hostname.example.com".into());
        assert_eq!(page(&s, 0)[0], "Host: a-very-long-hos");
    }

    #[test]
    fn sizes_are_formatted_like_df() {
        assert_eq!(human_size(0), "0B");
        assert_eq!(human_size(9 * GIB + GIB / 2), "9.5G");
        assert_eq!(human_size(11 * GIB + 1), "12G");
        assert_eq!(human_size(233 * GIB), "233G");
        assert_eq!(human_size(1536 * GIB), "1.5T");
        assert_eq!(human_size(1010 * GIB), "1.0T");
    }

    #[test]
    fn widest_lines_fit_the_panel() {
        let mut s = full();
        s.ip = Some((Kind::Ethernet, Ipv4Addr::BROADCAST));
        s.disk = Some(DiskUsage {
            used: 999 * GIB,
            avail: 0,
            total: 999 * GIB,
        });
        s.cpu = Some(100.0);
        s.mem = Some(100.0);
        s.temp = Some(105.25);
        s.host = Some("x".repeat(64));
        for line in page(&s, 0).iter().chain(&page(&s, 1)) {
            assert!(line.chars().count() <= COLUMNS, "{line:?} is too wide");
        }
    }

    #[test]
    fn missing_bus_is_reported() {
        let err = check_bus(Path::new("/dev/i2c-does-not-exist"));
        assert!(matches!(err, Err(Error::BusNotFound(_))));
    }
}
