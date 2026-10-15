//! How a browser reaches this server, and therefore whether the three-leg
//! measurement (device ↔ server, server ↔ internet, device ↔ internet) can be
//! composed into a diagnosis.
//!
//! Classified from the client's source address against this box's interface
//! networks and default routes:
//!
//! | relation   | meaning                                                         |
//! |------------|-----------------------------------------------------------------|
//! | `gateway`  | the device's internet traffic passes through this server        |
//! | `same-lan` | the server is a neighbour of the device, both behind one router |
//! | `routed`   | private address outside our subnets (VLAN, guest network, VPN)  |
//! | `hairpin`  | the request came back in through the router's NAT (public URL)  |
//! | `remote`   | a public address: the device is somewhere else on the internet  |
//! | `self`     | the page was opened on this machine                              |
//!
//! `ip_forward` alone is not enough to call a box a gateway: every Docker
//! host forwards. A gateway also needs the client on a different interface
//! than the default route.
//!
//! This is a pre-run guess. After the run, the page compares the public
//! addresses that the public server saw for this server and for the browser.

use std::net::{IpAddr, Ipv4Addr};

use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Gateway,
    SameLan,
    Routed,
    Hairpin,
    Remote,
    SelfHost,
}

impl Relation {
    pub fn as_str(self) -> &'static str {
        match self {
            Relation::Gateway => "gateway",
            Relation::SameLan => "same-lan",
            Relation::Routed => "routed",
            Relation::Hairpin => "hairpin",
            Relation::Remote => "remote",
            Relation::SelfHost => "self",
        }
    }

    /// Whether the device's internet traffic shares this server's line, so
    /// the three legs can be compared. A browser on the server itself does
    /// (its local leg is just loopback).
    pub fn legs_valid(self) -> bool {
        matches!(self, Relation::Gateway | Relation::SameLan | Relation::Routed | Relation::SelfHost)
    }
}

/// One address of a local interface.
#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    pub iface: String,
    pub ip: IpAddr,
    pub prefix: u8,
}

/// Snapshot of this box's networking, enough to classify a client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetView {
    pub nets: Vec<Net>,
    /// Interface and next hop of the IPv4 default route.
    pub default_v4: Option<(String, Option<Ipv4Addr>)>,
    pub default_v6: Option<String>,
    pub forward_v4: bool,
    pub forward_v6: bool,
}

/// Plain IPv4 for IPv4-mapped IPv6 addresses.
pub fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
        ip => ip,
    }
}

/// Private, loopback, link-local and unique-local addresses. Not CGNAT
/// (100.64/10): on a gateway with a CGNAT uplink those are other subscribers.
pub fn is_local_ip(ip: IpAddr) -> bool {
    match canonical(ip) {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let seg0 = v6.segments()[0];
            v6.is_loopback() || (seg0 & 0xfe00) == 0xfc00 || (seg0 & 0xffc0) == 0xfe80
        }
    }
}

/// 100.64.0.0/10: carrier-grade NAT, the provider shares the public address.
pub fn is_cgnat(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && (o[1] & 0xc0) == 64
}

fn in_net(ip: IpAddr, net: &Net) -> bool {
    match (ip, net.ip) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let p = net.prefix.min(32) as u32;
            let mask = if p == 0 { 0 } else { u32::MAX << (32 - p) };
            (u32::from(a) & mask) == (u32::from(b) & mask)
        }
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            let p = net.prefix.min(128) as u32;
            let mask = if p == 0 { 0 } else { u128::MAX << (128 - p) };
            (u128::from(a) & mask) == (u128::from(b) & mask)
        }
        _ => false,
    }
}

impl NetView {
    /// The current view, from `getifaddrs` and (on Linux) `/proc`.
    pub fn current() -> Self {
        let mut nets = Vec::new();
        if let Ok(ifaces) = if_addrs::get_if_addrs() {
            for i in ifaces {
                let (ip, prefix) = match &i.addr {
                    if_addrs::IfAddr::V4(a) => (IpAddr::V4(a.ip), a.prefixlen),
                    if_addrs::IfAddr::V6(a) => (IpAddr::V6(a.ip), a.prefixlen),
                };
                nets.push(Net { iface: i.name.clone(), ip, prefix });
            }
        }
        let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
        NetView {
            nets,
            default_v4: parse_route_v4(&read("/proc/net/route")),
            default_v6: parse_route_v6(&read("/proc/net/ipv6_route")),
            forward_v4: read("/proc/sys/net/ipv4/ip_forward").trim() == "1",
            forward_v6: read("/proc/sys/net/ipv6/conf/all/forwarding").trim() == "1",
        }
    }

    pub fn classify(&self, client: IpAddr) -> Relation {
        let client = canonical(client);
        if client.is_loopback() || self.nets.iter().any(|n| n.ip == client) {
            return Relation::SelfHost;
        }
        if let (IpAddr::V4(c), Some((_, Some(gw)))) = (client, &self.default_v4) {
            // Hairpin NAT rewrites the source to the router's own address.
            if c == *gw {
                return Relation::Hairpin;
            }
        }
        if !is_local_ip(client) {
            return Relation::Remote;
        }
        let Some(net) = self
            .nets
            .iter()
            .filter(|n| !n.ip.is_loopback() && in_net(client, n))
            .max_by_key(|n| n.prefix)
        else {
            return Relation::Routed;
        };
        let (default_iface, forwarding) = match client {
            IpAddr::V4(_) => (self.default_v4.as_ref().map(|d| d.0.as_str()), self.forward_v4),
            IpAddr::V6(_) => (self.default_v6.as_deref(), self.forward_v6),
        };
        match default_iface {
            Some(d) if d != net.iface && forwarding => Relation::Gateway,
            _ => Relation::SameLan,
        }
    }

    /// The uplink has a carrier-grade NAT address (only visible on the
    /// gateway itself).
    pub fn cgnat(&self) -> bool {
        let Some((iface, _)) = &self.default_v4 else { return false };
        self.nets
            .iter()
            .any(|n| &n.iface == iface && matches!(n.ip, IpAddr::V4(v4) if is_cgnat(v4)))
    }
}

/// Interface and gateway of the IPv4 default route in `/proc/net/route`.
pub fn parse_route_v4(text: &str) -> Option<(String, Option<Ipv4Addr>)> {
    text.lines().skip(1).find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() < 3 || f[1] != "00000000" {
            return None;
        }
        // Little-endian hex.
        let gw = u32::from_str_radix(f[2], 16)
            .ok()
            .map(|v| Ipv4Addr::from(v.swap_bytes()))
            .filter(|g| !g.is_unspecified());
        Some((f[0].to_string(), gw))
    })
}

/// Interface of the IPv6 default route in `/proc/net/ipv6_route`.
pub fn parse_route_v6(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        let default = f.len() >= 10 && f[0].chars().all(|c| c == '0') && f[1] == "00";
        // Skip the unreachable/blackhole default on the loopback device.
        (default && f[9] != "lo").then(|| f[9].to_string())
    })
}

pub fn hostname() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Whether two public addresses leave through the same line: identical IPv4
/// (one NAT), or the same /56 for IPv6 (one delegated prefix).
pub fn same_exit(a: IpAddr, b: IpAddr) -> bool {
    match (canonical(a), canonical(b)) {
        (IpAddr::V4(a), IpAddr::V4(b)) => a == b,
        (IpAddr::V6(a), IpAddr::V6(b)) => (u128::from(a) >> 72) == (u128::from(b) >> 72),
        _ => false,
    }
}

/// `/api/info` → `path`.
pub fn path_json(view: &NetView, client: Option<IpAddr>, lan_url: Option<String>) -> Value {
    let relation = client.map(|c| view.classify(c));
    json!({
        "relation": relation.map(Relation::as_str),
        "legsValid": relation.is_some_and(Relation::legs_valid),
        "cgnat": view.cgnat(),
        "hostname": hostname(),
        "lanUrl": lan_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(iface: &str, cidr: &str) -> Net {
        let (ip, p) = cidr.split_once('/').unwrap();
        Net { iface: iface.into(), ip: ip.parse().unwrap(), prefix: p.parse().unwrap() }
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    /// OpenWrt-style router: LAN bridge, WAN with a public address.
    fn router() -> NetView {
        NetView {
            nets: vec![
                net("lo", "127.0.0.1/8"),
                net("br-lan", "192.168.1.1/24"),
                net("br-lan", "fd00:1::1/64"),
                net("wan", "203.0.113.7/24"),
            ],
            default_v4: Some(("wan".into(), Some("203.0.113.1".parse().unwrap()))),
            default_v6: Some("wan".into()),
            forward_v4: true,
            forward_v6: true,
        }
    }

    /// NAS or Raspberry Pi behind the home router, Docker installed.
    fn nas() -> NetView {
        NetView {
            nets: vec![
                net("lo", "127.0.0.1/8"),
                net("eth0", "192.168.0.251/24"),
                net("docker0", "172.17.0.1/16"),
            ],
            default_v4: Some(("eth0".into(), Some("192.168.0.1".parse().unwrap()))),
            default_v6: None,
            forward_v4: true, // Docker turns this on
            forward_v6: false,
        }
    }

    #[test]
    fn gateway_vs_neighbour() {
        assert_eq!(router().classify(ip("192.168.1.23")), Relation::Gateway);
        assert_eq!(router().classify(ip("fd00:1::23")), Relation::Gateway);
        assert_eq!(router().classify(ip("::ffff:192.168.1.23")), Relation::Gateway);
        // A Docker host forwards but is not the device's gateway.
        assert_eq!(nas().classify(ip("192.168.0.12")), Relation::SameLan);
        // Same box without forwarding: neighbour.
        let mut r = router();
        r.forward_v4 = false;
        assert_eq!(r.classify(ip("192.168.1.23")), Relation::SameLan);
    }

    #[test]
    fn routed_remote_hairpin_self() {
        assert_eq!(nas().classify(ip("10.8.0.4")), Relation::Routed);
        assert_eq!(nas().classify(ip("198.51.100.20")), Relation::Remote);
        assert_eq!(nas().classify(ip("192.168.0.1")), Relation::Hairpin);
        assert_eq!(nas().classify(ip("127.0.0.1")), Relation::SelfHost);
        assert_eq!(nas().classify(ip("192.168.0.251")), Relation::SelfHost);
        assert!(Relation::Gateway.legs_valid() && Relation::Routed.legs_valid());
        assert!(Relation::SelfHost.legs_valid());
        assert!(!Relation::Remote.legs_valid() && !Relation::Hairpin.legs_valid());
    }

    #[test]
    fn cgnat_detection() {
        assert!(!router().cgnat());
        let mut r = router();
        r.nets.push(net("wan", "100.72.3.4/10"));
        assert!(r.cgnat());
        assert!(is_cgnat("100.64.0.1".parse().unwrap()) && !is_cgnat("100.128.0.1".parse().unwrap()));
    }

    #[test]
    fn route_parsing() {
        let v4 = "Iface\tDestination\tGateway \tFlags\nlan0\t0000A8C0\t00000000\t0001\neth0\t00000000\t0100A8C0\t0003\n";
        assert_eq!(parse_route_v4(v4), Some(("eth0".into(), Some("192.168.0.1".parse().unwrap()))));
        let v6 = "fd000001000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001 br-lan\n\
                  00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000400 00000001 00000000 00000003 wan\n";
        assert_eq!(parse_route_v6(v6).as_deref(), Some("wan"));
        let lo = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo\n";
        assert_eq!(parse_route_v6(lo), None);
    }

    #[test]
    fn exit_comparison() {
        assert!(same_exit(ip("203.0.113.7"), ip("::ffff:203.0.113.7")));
        assert!(!same_exit(ip("203.0.113.7"), ip("203.0.113.8")));
        assert!(same_exit(ip("2001:db8:aa:10::1"), ip("2001:db8:aa:20::9")));
        assert!(!same_exit(ip("2001:db8:aa:10::1"), ip("2001:db8:ab:10::1")));
        assert!(!same_exit(ip("203.0.113.7"), ip("2001:db8::1")));
    }
}
