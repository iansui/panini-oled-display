mod error;
mod net;
mod screen;
mod stats;

use clap::Parser;
use error::Error;
use screen::{Panel, Snapshot};
use signal_hook::consts::{SIGINT, SIGTERM};
use stats::Sampler;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "panini-oled", about)]
struct Cli {
    /// I2C bus device the panel is on (header pins 3 and 5 on the Orin Nano)
    #[arg(long, default_value = "/dev/i2c-7", value_name = "DEVICE")]
    bus: PathBuf,

    /// 7-bit I2C address of the panel, in hex
    #[arg(long, default_value = "0x3c", value_parser = parse_address, value_name = "HEX")]
    address: u8,

    /// Seconds between updates of the readings
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..), value_name = "SECONDS")]
    interval: u64,

    /// Seconds each page stays on the panel before the next one
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..), value_name = "SECONDS")]
    page_seconds: u64,

    /// Show only this page (1: host, IP, disk; 2: temperature, CPU, RAM) instead of rotating
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..=2), value_name = "N")]
    page: Option<u64>,

    /// Mount point whose disk usage is shown
    #[arg(long, default_value = "/", value_name = "PATH")]
    disk: PathBuf,

    /// Thermal zone type to read the temperature from (see /sys/class/thermal/thermal_zone*/type)
    #[arg(long, default_value = "cpu-thermal", value_name = "TYPE")]
    thermal_zone: String,

    /// Draw one frame, leave it on the panel and exit
    #[arg(long)]
    once: bool,

    /// Print the lines to stdout instead of drawing them; needs no panel
    #[arg(long)]
    dry_run: bool,

    /// Blank the panel and exit
    #[arg(long, conflicts_with_all = ["once", "dry_run"])]
    clear: bool,
}

fn parse_address(s: &str) -> Result<u8, String> {
    let digits = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    match u8::from_str_radix(digits, 16) {
        Ok(addr) if (0x03..=0x77).contains(&addr) => Ok(addr),
        _ => Err(format!("{s:?} is not a 7-bit hex address such as 0x3c")),
    }
}

fn run(cli: Cli) -> Result<(), Error> {
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [SIGTERM, SIGINT] {
        signal_hook::flag::register(signal, Arc::clone(&stop)).map_err(Error::Signal)?;
    }

    if !cli.clear && stats::disk_usage(&cli.disk).is_none() {
        return Err(Error::Disk(cli.disk));
    }
    let mut panel = if cli.dry_run {
        None
    } else {
        Some(Panel::open(&cli.bus, cli.address)?)
    };
    if cli.clear {
        return panel.as_mut().map_or(Ok(()), Panel::clear);
    }

    let mut sampler = Sampler::new(&cli.thermal_zone);
    match sampler.temp_path() {
        Some(path) => eprintln!("temperature from {}", path.display()),
        None => eprintln!(
            "warning: no thermal zone of type {}, the temperature shows N/A",
            cli.thermal_zone
        ),
    }
    if panel.is_some() && !cli.once {
        eprintln!(
            "drawing on {} at {:#04x}, readings every {}s, pages every {}s",
            cli.bus.display(),
            cli.address,
            cli.interval,
            cli.page_seconds
        );
    }

    // The first CPU reading needs some elapsed time to measure.
    sleep_unless(&stop, Duration::from_millis(250));
    let interval = Duration::from_secs(cli.interval);
    let start = Instant::now();
    loop {
        let snap = snapshot(&mut sampler, &cli.disk);
        let n = match cli.page {
            Some(p) => p as usize - 1,
            None => page_at(start.elapsed(), cli.page_seconds),
        };
        let (marker, lines) = (screen::marker(n), screen::page(&snap, n));
        match panel.as_mut() {
            Some(panel) => panel.draw(&marker, &lines)?,
            None => println!("[{marker}]\n{}\n", lines.join("\n")),
        }
        if cli.once || !sleep_unless(&stop, interval) {
            break;
        }
    }

    if let Some(panel) = panel.as_mut().filter(|_| !cli.once) {
        panel.clear()?;
        eprintln!("stopped, panel cleared");
    }
    Ok(())
}

/// Which page is up after `elapsed`: each one stays for `page_seconds`, then the next.
fn page_at(elapsed: Duration, page_seconds: u64) -> usize {
    (elapsed.as_secs() / page_seconds) as usize % screen::PAGES
}

fn snapshot(sampler: &mut Sampler, disk: &Path) -> Snapshot {
    Snapshot {
        host: stats::hostname(),
        ip: net::current(),
        disk: stats::disk_usage(disk),
        temp: sampler.temp(),
        cpu: sampler.cpu(),
        mem: sampler.mem(),
    }
}

/// Sleep for `d` in short steps; returns false as soon as a stop signal has arrived.
fn sleep_unless(stop: &AtomicBool, d: Duration) -> bool {
    let deadline = Instant::now() + d;
    while !stop.load(Ordering::Relaxed) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        thread::sleep(left.min(Duration::from_millis(100)));
    }
    false
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("panini-oled").chain(args.iter().copied()))
    }

    #[test]
    fn defaults_match_the_panini_case() {
        let cli = parse(&[]).unwrap();
        assert_eq!(cli.bus, PathBuf::from("/dev/i2c-7"));
        assert_eq!(cli.address, 0x3c);
        assert_eq!(cli.interval, 1);
        assert_eq!(cli.thermal_zone, "cpu-thermal");
        assert_eq!(cli.page_seconds, 10);
        assert_eq!(cli.page, None);
        assert_eq!(cli.disk, PathBuf::from("/"));
    }

    #[test]
    fn pages_alternate_every_page_seconds() {
        let at = |secs| page_at(Duration::from_secs(secs), 10);
        assert_eq!([at(0), at(9), at(10), at(19), at(20)], [0, 0, 1, 1, 0]);
    }

    #[test]
    fn out_of_range_page_options_are_rejected() {
        assert!(parse(&["--page", "0"]).is_err());
        assert!(parse(&["--page", "3"]).is_err());
        assert!(parse(&["--page-seconds", "0"]).is_err());
        assert_eq!(parse(&["--page", "2"]).unwrap().page, Some(2));
    }

    #[test]
    fn missing_disk_path_is_reported() {
        let cli = parse(&["--dry-run", "--once", "--disk", "/nonexistent/panini-oled"]).unwrap();
        assert!(matches!(run(cli), Err(Error::Disk(_))));
    }

    #[test]
    fn addresses_are_hex_with_or_without_prefix() {
        assert_eq!(parse_address("0x3c"), Ok(0x3c));
        assert_eq!(parse_address("3D"), Ok(0x3d));
        assert!(parse_address("0x80").is_err());
        assert!(parse_address("60").is_ok());
        assert!(parse_address("zz").is_err());
        assert!(parse_address("0x0x3c").is_err());
    }

    #[test]
    fn clear_cannot_be_combined_with_drawing() {
        assert!(parse(&["--clear", "--once"]).is_err());
        assert!(parse(&["--clear", "--dry-run"]).is_err());
    }

    #[test]
    fn zero_interval_is_rejected() {
        assert!(parse(&["--interval", "0"]).is_err());
    }

    #[test]
    fn sleep_stops_early_on_signal() {
        let stop = AtomicBool::new(true);
        let start = Instant::now();
        assert!(!sleep_unless(&stop, Duration::from_secs(5)));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(sleep_unless(
            &AtomicBool::new(false),
            Duration::from_millis(10)
        ));
    }
}
