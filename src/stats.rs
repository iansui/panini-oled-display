//! CPU use, memory use, temperature, disk use and hostname, read straight from `/proc`, `/sys`
//! and `statvfs`.

use std::fs;
use std::path::{Path, PathBuf};

const PROC_STAT: &str = "/proc/stat";
const PROC_MEMINFO: &str = "/proc/meminfo";
const THERMAL_ROOT: &str = "/sys/class/thermal";
const PROC_HOSTNAME: &str = "/proc/sys/kernel/hostname";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CpuTimes {
    busy: u64,
    total: u64,
}

/// The aggregate `cpu` line of `/proc/stat`. Idle and iowait both count as idle.
pub fn parse_cpu_times(stat: &str) -> Option<CpuTimes> {
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .map(|f| f.parse().ok())
        .collect::<Option<_>>()?;
    // guest and guest_nice (fields 9 and 10) are already included in user and nice.
    let total: u64 = fields.iter().take(8).sum();
    let idle = fields.get(3)? + fields.get(4).copied().unwrap_or(0);
    Some(CpuTimes {
        busy: total.saturating_sub(idle),
        total,
    })
}

pub fn cpu_percent(prev: CpuTimes, cur: CpuTimes) -> f32 {
    let total = cur.total.saturating_sub(prev.total);
    if total == 0 {
        return 0.0;
    }
    let busy = cur.busy.saturating_sub(prev.busy);
    (busy as f32 / total as f32 * 100.0).clamp(0.0, 100.0)
}

/// `(MemTotal - MemAvailable) / MemTotal`, so page cache that can be reclaimed counts as free.
pub fn parse_mem_percent(meminfo: &str) -> Option<f32> {
    let field = |name: &str| -> Option<u64> {
        let line = meminfo.lines().find(|l| l.starts_with(name))?;
        line[name.len()..].split_whitespace().next()?.parse().ok()
    };
    let total = field("MemTotal:")?;
    let available = field("MemAvailable:")?;
    if total == 0 {
        return None;
    }
    Some(total.saturating_sub(available) as f32 / total as f32 * 100.0)
}

/// The `temp` file of the first `thermal_zone*` under `root` whose `type` is `kind`.
pub fn find_thermal_zone(root: &Path, kind: &str) -> Option<PathBuf> {
    let mut zones: Vec<PathBuf> = fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("thermal_zone"))
        })
        .collect();
    zones.sort();
    zones
        .into_iter()
        .find(|z| fs::read_to_string(z.join("type")).is_ok_and(|t| t.trim() == kind))
        .map(|z| z.join("temp"))
}

/// Millidegrees Celsius, as the kernel writes them, to degrees.
pub fn parse_temp(raw: &str) -> Option<f32> {
    raw.trim().parse::<i64>().ok().map(|m| m as f32 / 1000.0)
}

pub struct Sampler {
    prev: Option<CpuTimes>,
    temp_path: Option<PathBuf>,
}

impl Sampler {
    pub fn new(thermal_zone: &str) -> Self {
        Sampler {
            prev: read_cpu_times(),
            temp_path: find_thermal_zone(Path::new(THERMAL_ROOT), thermal_zone),
        }
    }

    pub fn temp_path(&self) -> Option<&Path> {
        self.temp_path.as_deref()
    }

    /// CPU use since the previous call (or since `new` on the first call).
    pub fn cpu(&mut self) -> Option<f32> {
        let cur = read_cpu_times()?;
        let percent = self.prev.map(|prev| cpu_percent(prev, cur));
        self.prev = Some(cur);
        percent
    }

    pub fn mem(&self) -> Option<f32> {
        parse_mem_percent(&fs::read_to_string(PROC_MEMINFO).ok()?)
    }

    pub fn temp(&self) -> Option<f32> {
        parse_temp(&fs::read_to_string(self.temp_path.as_ref()?).ok()?)
    }
}

fn read_cpu_times() -> Option<CpuTimes> {
    parse_cpu_times(&fs::read_to_string(PROC_STAT).ok()?)
}

/// Sizes in bytes, with the same meaning as the columns of `df`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiskUsage {
    pub used: u64,
    pub avail: u64,
    pub total: u64,
}

impl DiskUsage {
    /// `used / (used + avail)`, rounded up, as `df` computes `Use%`. Blocks reserved for root
    /// count as neither, so this is higher than `used / total`.
    pub fn percent(&self) -> u64 {
        let denom = self.used + self.avail;
        if denom == 0 {
            0
        } else {
            (self.used * 100).div_ceil(denom)
        }
    }
}

pub fn disk_usage(path: &Path) -> Option<DiskUsage> {
    let st = nix::sys::statvfs::statvfs(path).ok()?;
    let frsize = st.fragment_size() as u64;
    let blocks = st.blocks() as u64;
    Some(DiskUsage {
        used: blocks.saturating_sub(st.blocks_free() as u64) * frsize,
        avail: st.blocks_available() as u64 * frsize,
        total: blocks * frsize,
    })
}

pub fn hostname() -> Option<String> {
    let name = fs::read_to_string(PROC_HOSTNAME).ok()?;
    Some(name.trim().to_string()).filter(|n| !n.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 50 0 25 400 25 0 0 0 0 0\nintr 1\n";

    #[test]
    fn aggregate_cpu_line_is_parsed() {
        let t = parse_cpu_times(STAT).unwrap();
        assert_eq!(
            t,
            CpuTimes {
                busy: 150,
                total: 1000
            }
        );
    }

    #[test]
    fn guest_time_is_not_counted_twice() {
        let t = parse_cpu_times("cpu  100 0 0 100 0 0 0 0 40 10\n").unwrap();
        assert_eq!(t.total, 200);
    }

    #[test]
    fn missing_cpu_line_is_none() {
        assert_eq!(parse_cpu_times("intr 1\n"), None);
        assert_eq!(parse_cpu_times("cpu  1 x 3 4\n"), None);
    }

    #[test]
    fn cpu_percent_uses_the_delta() {
        let prev = CpuTimes {
            busy: 100,
            total: 1000,
        };
        let cur = CpuTimes {
            busy: 150,
            total: 1200,
        };
        assert_eq!(cpu_percent(prev, cur), 25.0);
    }

    #[test]
    fn cpu_percent_without_elapsed_time_is_zero() {
        let t = CpuTimes { busy: 5, total: 10 };
        assert_eq!(cpu_percent(t, t), 0.0);
    }

    #[test]
    fn mem_percent_uses_available_memory() {
        let info =
            "MemTotal:        8000 kB\nMemFree:          1000 kB\nMemAvailable:     6000 kB\n";
        assert_eq!(parse_mem_percent(info), Some(25.0));
        assert_eq!(parse_mem_percent("MemTotal: 8000 kB\n"), None);
    }

    #[test]
    fn temp_is_converted_from_millidegrees() {
        assert_eq!(parse_temp("53625\n"), Some(53.625));
        assert_eq!(parse_temp(""), None);
    }

    #[test]
    fn disk_percent_rounds_up_like_df() {
        let d = DiskUsage {
            used: 12,
            avail: 210,
            total: 233,
        };
        assert_eq!(d.percent(), 6);
        let full = DiskUsage {
            used: 10,
            avail: 0,
            total: 11,
        };
        assert_eq!(full.percent(), 100);
        let empty = DiskUsage {
            used: 0,
            avail: 0,
            total: 0,
        };
        assert_eq!(empty.percent(), 0);
    }

    #[test]
    fn root_disk_usage_is_consistent() {
        let d = disk_usage(Path::new("/")).unwrap();
        assert!(d.used + d.avail <= d.total);
        assert!(d.total > 0);
        assert_eq!(disk_usage(Path::new("/nonexistent/panini-oled")), None);
    }

    #[test]
    fn thermal_zone_is_found_by_type() {
        let dir = tempfile::tempdir().unwrap();
        for (zone, kind) in [
            ("thermal_zone0", "cpu-thermal"),
            ("thermal_zone1", "gpu-thermal"),
        ] {
            let z = dir.path().join(zone);
            fs::create_dir(&z).unwrap();
            fs::write(z.join("type"), format!("{kind}\n")).unwrap();
        }
        fs::create_dir(dir.path().join("cooling_device0")).unwrap();
        assert_eq!(
            find_thermal_zone(dir.path(), "gpu-thermal"),
            Some(dir.path().join("thermal_zone1/temp"))
        );
        assert_eq!(find_thermal_zone(dir.path(), "tj-thermal"), None);
    }
}
