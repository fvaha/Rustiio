//! Mrezni pomocnici: koja je nasa LAN adresa i je li adresa upotrebljiva.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// Primarna LAN IPv4 adresa (ona s koje izlazi default ruta).
///
/// Trik bez ijedne poslane paketa: UDP `connect` samo postavi lokalnu adresu.
pub fn primary_ipv4() -> Option<Ipv4Addr> {
    for probe in ["1.1.1.1:80", "8.8.8.8:80"] {
        let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else { continue };
        if sock.connect(probe).is_err() {
            continue;
        }
        if let Ok(SocketAddr::V4(addr)) = sock.local_addr() {
            let ip = *addr.ip();
            if !ip.is_loopback() && !ip.is_unspecified() {
                return Some(ip);
            }
        }
    }
    None
}

/// Parsiraj IP iz configa; vrati `None` za prazno/neispravno.
pub fn parse_ip(value: &str) -> Option<IpAddr> {
    value.trim().parse::<IpAddr>().ok()
}

/// Je li adresa upotrebljiva za oglasavanje u mrezi (nije loopback/link-local).
pub fn is_lan_capable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_unspecified() && !v4.is_link_local(),
        IpAddr::V6(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ip_accepts_v4() {
        assert_eq!(parse_ip("192.168.1.10"), Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))));
        assert_eq!(parse_ip("  "), None);
        assert_eq!(parse_ip("nije-ip"), None);
    }

    #[test]
    fn lan_capable_rejects_loopback() {
        assert!(!is_lan_capable(&IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(is_lan_capable(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))));
    }

    #[test]
    fn primary_ipv4_is_usable_when_online() {
        // U CI-ju bez mreze moze biti None; ako postoji, mora biti upotrebljiva.
        if let Some(ip) = primary_ipv4() {
            assert!(is_lan_capable(&IpAddr::V4(ip)), "dobili smo {ip}, a to nije LAN adresa");
        }
    }
}
