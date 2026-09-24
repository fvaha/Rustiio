//! SSDP probe — posalje `M-SEARCH` i skupi odgovore.
//!
//! Ovo je nas glavni alat za dijagnostiku: njime dokazujemo da nas TV-i vide,
//! ali i da vidimo sto Serviio (i drugi) oglasavaju u istoj mrezi.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::time::{sleep, timeout};
use tracing::debug;

use crate::message::{Message, MessageKind, build_msearch};
use crate::{MULTICAST_ADDR, MULTICAST_PORT};

/// Jedan otkriveni uredaj/servis (jedan `USN` moze dati vise targeta).
#[derive(Debug, Clone)]
pub struct Discovered {
    pub address: SocketAddr,
    pub st: String,
    pub usn: String,
    pub location: String,
    pub server: String,
    pub kind: MessageKind,
}

impl Discovered {
    /// Stabilan kljuc za dedupe: USN ako ga ima, inace lokacija + target.
    fn key(&self) -> String {
        if self.usn.is_empty() {
            format!("{}|{}", self.location, self.st)
        } else {
            format!("{}|{}", self.usn, self.st)
        }
    }

    /// Je li ovo najava prestanka rada (Serviio/neki server salje i byebye).
    pub fn is_byebye(&self) -> bool {
        false
    }

    /// Kratki opis za ispis u CLI-ju.
    pub fn summary(&self) -> String {
        let name = if self.location.is_empty() { "-" } else { self.location.as_str() };
        format!("{:16} {:38} {}", self.address.ip().to_string(), self.st, name)
    }
}

/// Posalji `M-SEARCH` (3 puta) i cekaj `wait` — vrati jedinstvene odgovore.
pub async fn discover(wait: Duration) -> anyhow::Result<Vec<Discovered>> {
    discover_target("ssdp:all", wait).await
}

/// Kao [`discover`], ali za odredeni `ST`.
pub async fn discover_target(search_target: &str, wait: Duration) -> anyhow::Result<Vec<Discovered>> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).await?;
    socket.set_multicast_ttl_v4(4)?;
    let dest: SocketAddr = SocketAddrV4::new(Ipv4Addr::from(MULTICAST_ADDR), MULTICAST_PORT).into();

    for _ in 0..3 {
        socket.send_to(build_msearch(search_target, 2).as_bytes(), dest).await?;
        sleep(Duration::from_millis(150)).await;
    }

    let deadline = Instant::now() + wait;
    let mut found: HashMap<String, Discovered> = HashMap::new();
    let mut buf = vec![0u8; 4096];

    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        let slice = remaining.min(Duration::from_millis(250));
        match timeout(slice, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, from))) => {
                let text = String::from_utf8_lossy(&buf[..len]).to_string();
                let Some(msg) = Message::parse(&text) else { continue };
                if msg.kind == MessageKind::Search {
                    continue; // nas vlastiti M-SEARCH (multicast loop)
                }
                let item = Discovered {
                    address: from,
                    st: msg.search_target().unwrap_or_default().to_string(),
                    usn: msg.usn().unwrap_or_default().to_string(),
                    location: msg.location().unwrap_or_default().to_string(),
                    server: msg.server().unwrap_or_default().to_string(),
                    kind: msg.kind,
                };
                debug!(usn = %item.usn, st = %item.st, "SSDP odgovor");
                found.entry(item.key()).or_insert(item);
            }
            Ok(Err(err)) => debug!(error = %err, "recv_from u probeu"),
            Err(_) => {} // timeout -> provjeri deadline
        }
    }

    let mut out: Vec<Discovered> = found.into_values().collect();
    out.sort_by(|a, b| (a.address.ip(), &a.usn).cmp(&(b.address.ip(), &b.usn)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_prefers_usn() {
        let d = Discovered {
            address: "127.0.0.1:1900".parse().unwrap(),
            st: "upnp:rootdevice".into(),
            usn: "uuid:x::upnp:rootdevice".into(),
            location: String::new(),
            server: String::new(),
            kind: MessageKind::Response,
        };
        assert_eq!(d.key(), "uuid:x::upnp:rootdevice|upnp:rootdevice");
    }
}
