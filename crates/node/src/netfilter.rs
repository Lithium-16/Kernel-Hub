//! Only accept connections from this machine, the LAN, or the tailnet.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub fn is_allowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => allowed_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => allowed_v4(v4),
            None => allowed_v6(v6),
        },
    }
}

fn allowed_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private() // 10/8, 172.16/12, 192.168/16
        || (a == 100 && (64..=127).contains(&b)) // 100.64/10: CGNAT, used by Tailscale
}

fn allowed_v6(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 unique local, incl. Tailscale's fd7a:115c:a1e0::/48
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_local_lan_and_tailnet() {
        for ip in [
            "127.0.0.1",
            "10.0.0.5",
            "172.20.1.1",
            "192.168.1.20",
            "100.101.12.4",
            "::1",
            "fd7a:115c:a1e0::1",
            "::ffff:192.168.1.2",
        ] {
            assert!(is_allowed(ip.parse().unwrap()), "{ip} should be allowed");
        }
    }

    #[test]
    fn rejects_public_addresses() {
        for ip in [
            "8.8.8.8",
            "172.32.0.1",
            "100.128.0.1",
            "1.1.1.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(!is_allowed(ip.parse().unwrap()), "{ip} should be rejected");
        }
    }
}
