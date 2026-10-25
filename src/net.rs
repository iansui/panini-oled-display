//! Picks the one IPv4 address worth showing: Wi-Fi first, then Ethernet, then anything else.

use std::net::Ipv4Addr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Wifi,
    Ethernet,
    Other,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Wifi => "WiFi",
            Kind::Ethernet => "Eth",
            Kind::Other => "IP",
        }
    }
}

pub struct Candidate {
    pub name: String,
    pub ip: Ipv4Addr,
    pub up: bool,
}

/// Kernel names start with `wl` for Wi-Fi (`wlan0`, `wlP1p1s0`) and `en`/`eth` for Ethernet.
fn kind(name: &str) -> Kind {
    if name.starts_with("wl") {
        Kind::Wifi
    } else if name.starts_with("en") || name.starts_with("eth") {
        Kind::Ethernet
    } else {
        Kind::Other
    }
}

/// The best address among interfaces that are up. Ties keep the kernel's order.
pub fn pick(candidates: &[Candidate]) -> Option<(Kind, Ipv4Addr)> {
    candidates
        .iter()
        .filter(|c| c.up && !c.ip.is_loopback() && !c.ip.is_link_local())
        .map(|c| (kind(&c.name), c.ip))
        .min_by_key(|(k, _)| *k)
}

pub fn current() -> Option<(Kind, Ipv4Addr)> {
    let candidates: Vec<Candidate> = if_addrs::get_if_addrs()
        .ok()?
        .into_iter()
        .filter_map(|iface| match iface.addr {
            if_addrs::IfAddr::V4(ref v4) => Some(Candidate {
                ip: v4.ip,
                up: iface.is_oper_up(),
                name: iface.name,
            }),
            if_addrs::IfAddr::V6(_) => None,
        })
        .collect();
    pick(&candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(name: &str, ip: [u8; 4], up: bool) -> Candidate {
        Candidate {
            name: name.into(),
            ip: Ipv4Addr::from(ip),
            up,
        }
    }

    #[test]
    fn jetson_wifi_name_counts_as_wifi() {
        let ifaces = [
            c("lo", [127, 0, 0, 1], true),
            c("l4tbr0", [198, 51, 100, 1], false),
            c("docker0", [198, 51, 100, 2], false),
            c("wg0", [203, 0, 113, 5], true),
            c("wlP1p1s0", [192, 0, 2, 10], true),
        ];
        assert_eq!(
            pick(&ifaces),
            Some((Kind::Wifi, Ipv4Addr::new(192, 0, 2, 10)))
        );
    }

    #[test]
    fn wifi_wins_over_ethernet() {
        let ifaces = [
            c("enP8p1s0", [192, 0, 2, 20], true),
            c("wlan0", [192, 0, 2, 10], true),
        ];
        assert_eq!(pick(&ifaces).map(|(k, _)| k), Some(Kind::Wifi));
    }

    #[test]
    fn ethernet_is_used_when_wifi_is_down() {
        let ifaces = [
            c("wlan0", [192, 0, 2, 10], false),
            c("eth0", [192, 0, 2, 20], true),
        ];
        assert_eq!(
            pick(&ifaces),
            Some((Kind::Ethernet, Ipv4Addr::new(192, 0, 2, 20)))
        );
    }

    #[test]
    fn other_interfaces_are_the_fallback() {
        let ifaces = [
            c("docker0", [198, 51, 100, 2], false),
            c("wg0", [203, 0, 113, 5], true),
        ];
        assert_eq!(
            pick(&ifaces),
            Some((Kind::Other, Ipv4Addr::new(203, 0, 113, 5)))
        );
    }

    #[test]
    fn loopback_and_link_local_are_skipped() {
        let ifaces = [
            c("lo", [127, 0, 0, 1], true),
            c("eth0", [169, 254, 0, 1], true),
        ];
        assert_eq!(pick(&ifaces), None);
    }
}
