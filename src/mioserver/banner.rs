//! Startup banner: tells the user where to open the browser client.
//!
//! ```text
//!   Nettest is running. Open the speed test on any device on this network:
//!
//!     http://192.168.1.10:5005
//!     http://nettest.local:5005
//!
//!   [QR code for the first address]
//! ```

use std::io::IsTerminal;
use std::net::{IpAddr, SocketAddr};

use crate::mioserver::server::ServerConfig;

/// URL for `ip:port` over `scheme`, omitting the default port.
fn url(scheme: &str, host: &str, port: u16) -> String {
    let default = (scheme == "http" && port == 80) || (scheme == "https" && port == 443);
    if default {
        format!("{}://{}/", scheme, host)
    } else {
        format!("{}://{}:{}/", scheme, host, port)
    }
}

fn host_for(ip: IpAddr) -> String {
    match ip {
        IpAddr::V6(v6) => format!("[{}]", v6),
        IpAddr::V4(v4) => v4.to_string(),
    }
}

/// Interface names of container bridges, VMs and VPN tunnels: reachable from
/// this host only, or from the far end of a tunnel, rarely from the LAN.
fn is_virtual_iface(name: &str) -> bool {
    const PREFIXES: [&str; 14] = [
        "docker", "br-", "veth", "virbr", "vmnet", "vboxnet", "cni", "flannel",
        "lxc", "podman", "tun", "tap", "wg", "utun",
    ];
    PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Sort key, lower first: the address a phone on the same Wi-Fi most
/// likely reaches. Private IPv4 on a real subnet, then other IPv4 on a
/// subnet, then point-to-point (/32, typically VPN) and virtual interfaces.
fn ipv4_rank(name: &str, ip: std::net::Ipv4Addr, prefixlen: u8) -> u8 {
    let virt = is_virtual_iface(name) || prefixlen >= 31;
    match (virt, ip.is_private()) {
        (false, true) => 0,
        (false, false) => 1,
        (true, _) => 2,
    }
}

/// Addresses other devices can reach: the listener's own address if it is
/// bound to a specific IP, otherwise every non-loopback interface address
/// (IPv4 first, most likely LAN address first; IPv6 only global/unique-local
/// ones, link-local needs a zone).
fn reachable_ips(listen: &SocketAddr) -> Vec<IpAddr> {
    if !listen.ip().is_unspecified() {
        return vec![listen.ip()];
    }
    let mut v4: Vec<(u8, std::net::Ipv4Addr)> = Vec::new();
    let mut v6 = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() || !iface.is_oper_up() {
                continue;
            }
            match &iface.addr {
                if_addrs::IfAddr::V4(a) if !a.ip.is_link_local() && !a.ip.is_unspecified() => {
                    v4.push((ipv4_rank(&iface.name, a.ip, a.prefixlen), a.ip))
                }
                if_addrs::IfAddr::V6(a)
                    if listen.is_ipv6()
                        && !a.ip.is_unspecified()
                        && (a.ip.segments()[0] & 0xffc0) != 0xfe80
                        && !is_virtual_iface(&iface.name) =>
                {
                    v6.push(a.ip)
                }
                _ => {}
            }
        }
    }
    v4.sort();
    v4.dedup_by_key(|(_, ip)| *ip);
    v6.sort();
    v6.dedup();
    v4.into_iter()
        .map(|(_, ip)| IpAddr::V4(ip))
        .chain(v6.into_iter().map(IpAddr::V6))
        .collect()
}

/// Web UI URLs in display order: plain-HTTP LAN addresses, the mDNS name,
/// then HTTPS (only meaningful with a hostname matching the certificate).
pub fn web_ui_urls(config: &ServerConfig) -> Vec<String> {
    let mut urls = Vec::new();
    let tcp_port = config.tcp_addresses.first().map(|a| a.port());

    let mut ips: Vec<IpAddr> = Vec::new();
    for addr in &config.tcp_addresses {
        for ip in reachable_ips(addr) {
            if !ips.contains(&ip) {
                ips.push(ip);
            }
        }
    }
    if let Some(port) = tcp_port {
        for ip in &ips {
            urls.push(url("http", &host_for(*ip), port));
        }
        if config.enable_mdns {
            urls.push(url("http", "nettest.local", port));
        }
        if urls.is_empty() {
            urls.push(url("http", "localhost", port));
        }
    }

    let tls = config.cert_path.is_some() && config.key_path.is_some();
    if let (true, Some(addr)) = (tls, config.tls_addresses.first()) {
        let host = config.hostname.clone().or_else(|| ips.first().map(|ip| host_for(*ip)));
        if let Some(host) = host {
            urls.push(url("https", &host, addr.port()));
        }
    }
    urls
}

/// Terminal QR code using half-block characters (two modules per line).
/// Drawn light-on-dark with a quiet zone, which scans on dark and light
/// terminal themes alike.
fn qr_lines(data: &str) -> Option<Vec<String>> {
    let code = qrcode::QrCode::with_error_correction_level(data, qrcode::EcLevel::L).ok()?;
    let width = code.width();
    let colors = code.to_colors();
    let quiet = 2isize;
    let dark = |x: isize, y: isize| -> bool {
        if x < 0 || y < 0 || x >= width as isize || y >= width as isize {
            return false;
        }
        colors[y as usize * width + x as usize] == qrcode::Color::Dark
    };
    let size = width as isize;
    let mut lines = Vec::new();
    let mut y = -quiet;
    while y < size + quiet {
        let mut line = String::new();
        for x in -quiet..size + quiet {
            // Printed light where the module is light, so dark modules stay
            // the terminal background colour.
            let top = !dark(x, y);
            let bottom = !dark(x, y + 1);
            line.push(match (top, bottom) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        lines.push(line);
        y += 2;
    }
    Some(lines)
}

/// Prints where to open the web UI. The QR code is only drawn on a terminal.
pub fn print_startup_banner(config: &ServerConfig) {
    let urls = web_ui_urls(config);
    if urls.is_empty() {
        return;
    }
    let mut out = String::new();
    out.push_str("\n  Nettest is running. Open the speed test on any device on this network:\n\n");
    for u in &urls {
        out.push_str(&format!("    {}\n", u));
    }
    if std::io::stdout().is_terminal() {
        if let Some(lines) = qr_lines(&urls[0]) {
            out.push('\n');
            for l in lines {
                out.push_str("    ");
                out.push_str(&l);
                out.push('\n');
            }
        }
    }
    out.push_str("\n  Press Ctrl+C to stop.\n");
    println!("{}", out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_omits_default_port() {
        assert_eq!(url("http", "1.2.3.4", 80), "http://1.2.3.4/");
        assert_eq!(url("http", "1.2.3.4", 5005), "http://1.2.3.4:5005/");
        assert_eq!(url("https", "x", 443), "https://x/");
    }

    #[test]
    fn specific_bind_address_is_used_as_is() {
        let addr: SocketAddr = "10.0.0.5:5005".parse().unwrap();
        assert_eq!(reachable_ips(&addr), vec!["10.0.0.5".parse::<IpAddr>().unwrap()]);
    }

    #[test]
    fn lan_address_ranks_before_tunnels_and_bridges() {
        let lan = ipv4_rank("wlan0", "192.168.0.251".parse().unwrap(), 24);
        let vpn = ipv4_rank("core", "10.35.3.2".parse().unwrap(), 32);
        let docker = ipv4_rank("docker0", "172.17.0.1".parse().unwrap(), 16);
        let public = ipv4_rank("eth0", "81.2.3.4".parse().unwrap(), 24);
        assert!(lan < public && public < vpn && vpn == docker);
    }

    #[test]
    fn qr_code_renders_square() {
        let lines = qr_lines("http://192.168.100.100:5005/").unwrap();
        let w = lines[0].chars().count();
        assert!(w >= 25);
        assert!(lines.iter().all(|l| l.chars().count() == w));
        assert_eq!(lines.len(), (w + 1) / 2);
    }
}
