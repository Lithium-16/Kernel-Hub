//! Who may connect: this machine and the tailnet, plus the home network only when
//! `allow_lan = true`. Everything else is refused before the WebSocket opens.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub fn is_allowed(ip: IpAddr, allow_lan: bool) -> bool {
    match ip {
        IpAddr::V4(v4) => allowed_v4(v4, allow_lan),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => allowed_v4(v4, allow_lan),
            None => allowed_v6(v6, allow_lan),
        },
    }
}

fn allowed_v4(ip: Ipv4Addr, allow_lan: bool) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || (a == 100 && (64..=127).contains(&b)) // 100.64/10: Tailscale addresses
        || (allow_lan && ip.is_private()) // 10/8, 172.16/12, 192.168/16
}

fn allowed_v6(ip: Ipv6Addr, allow_lan: bool) -> bool {
    let s = ip.segments();
    ip.is_loopback()
        || (s[0], s[1], s[2]) == (0xfd7a, 0x115c, 0xa1e0) // Tailscale's fd7a:115c:a1e0::/48
        || (allow_lan && (s[0] & 0xfe00) == 0xfc00) // fc00::/7 unique local
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(ip: &str, lan: bool) -> bool {
        is_allowed(ip.parse().unwrap(), lan)
    }

    #[test]
    fn tailnet_and_this_machine_always() {
        for ip in [
            "127.0.0.1",
            "100.101.12.4",
            "::1",
            "fd7a:115c:a1e0::1",
            "::ffff:100.64.0.9",
        ] {
            assert!(allowed(ip, false), "{ip} should be allowed");
        }
    }

    #[test]
    fn home_network_only_when_allowed() {
        for ip in [
            "10.0.0.5",
            "172.20.1.1",
            "192.168.1.20",
            "fd00::5",
            "::ffff:192.168.1.2",
        ] {
            assert!(!allowed(ip, false), "{ip} should be refused by default");
            assert!(allowed(ip, true), "{ip} should be allowed with allow_lan");
        }
    }

    #[test]
    fn never_the_internet() {
        for ip in [
            "8.8.8.8",
            "172.32.0.1",
            "100.128.0.1",
            "1.1.1.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(!allowed(ip, true), "{ip} should be refused");
        }
    }
}
