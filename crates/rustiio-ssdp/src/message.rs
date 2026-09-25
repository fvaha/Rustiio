//! SSDP poruke: `M-SEARCH`, `NOTIFY` i `HTTP/1.1 200 OK` odgovori.
//!
//! Sve je tekst, pa parser tolerira i `\n` i `\r\n`, neosjetljiv je na velicinu
//! slova u imenima headera i ignorira nepoznate headere (kako spec i nalaze).

use std::net::Ipv4Addr;

use crate::{MULTICAST_ADDR, MULTICAST_PORT};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// Klijent pita "ima li koga" (`M-SEARCH * HTTP/1.1`).
    Search,
    /// Nas periodicni oglas (`NOTIFY * HTTP/1.1`).
    Notify,
    /// Odgovor (`HTTP/1.1 200 OK`).
    Response,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub kind: MessageKind,
    /// Imena headera su normalizirana na mala slova.
    pub headers: Vec<(String, String)>,
}

impl Message {
    pub fn parse(data: &str) -> Option<Self> {
        let data = data.trim_start_matches('\u{feff}');
        let mut lines = data.split('\n').map(|l| l.trim_end_matches('\r'));

        let start = lines.next()?.trim();
        let upper = start.to_ascii_uppercase();
        let kind = if upper.starts_with("M-SEARCH") {
            MessageKind::Search
        } else if upper.starts_with("NOTIFY") {
            MessageKind::Notify
        } else if upper.starts_with("HTTP/1.1 200") || upper.starts_with("HTTP/1.0 200") {
            MessageKind::Response
        } else {
            return None;
        };

        let mut headers = Vec::new();
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (name, value) = line.split_once(':')?;
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }

        Some(Self { kind, headers })
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }

    /// `ST` (search target) ili `NT` (notification type).
    pub fn search_target(&self) -> Option<&str> {
        self.header("st").or_else(|| self.header("nt"))
    }

    pub fn usn(&self) -> Option<&str> {
        self.header("usn")
    }

    pub fn location(&self) -> Option<&str> {
        self.header("location")
    }

    pub fn server(&self) -> Option<&str> {
        self.header("server")
    }

    /// `MX` iz M-SEARCH (koliko sekundi klijent ceka odgovor).
    pub fn mx(&self) -> u32 {
        self.header("mx").and_then(|v| v.trim().parse().ok()).unwrap_or(1).clamp(1, 5)
    }

    pub fn nts(&self) -> Option<&str> {
        self.header("nts")
    }

    pub fn is_byebye(&self) -> bool {
        matches!(self.nts(), Some(v) if v.eq_ignore_ascii_case("ssdp:byebye"))
    }
}

/// `M-SEARCH * HTTP/1.1` upit (koristi ga `probe`).
pub fn build_msearch(search_target: &str, mx: u32) -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: {host}:{port}\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: {mx}\r\n\
         ST: {st}\r\n\r\n",
        host = Ipv4Addr::from(MULTICAST_ADDR),
        port = MULTICAST_PORT,
        mx = mx.clamp(1, 5),
        st = search_target,
    )
}

#[derive(Debug, Clone)]
pub struct NotifyParams<'a> {
    /// `ssdp:alive` ili `ssdp:byebye`.
    pub nts: &'a str,
    pub nt: &'a str,
    pub usn: &'a str,
    pub location: &'a str,
    pub server: &'a str,
    pub max_age: u32,
    pub boot_id: u32,
    pub config_id: u32,
}

pub fn build_notify(p: &NotifyParams<'_>) -> String {
    format!(
        "NOTIFY * HTTP/1.1\r\n\
         HOST: {host}:{port}\r\n\
         CACHE-CONTROL: max-age={max_age}\r\n\
         LOCATION: {location}\r\n\
         NT: {nt}\r\n\
         NTS: {nts}\r\n\
         SERVER: {server}\r\n\
         USN: {usn}\r\n\
         BOOTID.UPNP.ORG: {boot_id}\r\n\
         CONFIGID.UPNP.ORG: {config_id}\r\n\r\n",
        host = Ipv4Addr::from(MULTICAST_ADDR),
        port = MULTICAST_PORT,
        max_age = p.max_age,
        location = p.location,
        nt = p.nt,
        nts = p.nts,
        server = p.server,
        usn = p.usn,
        boot_id = p.boot_id,
        config_id = p.config_id,
    )
}

#[derive(Debug, Clone)]
pub struct ResponseParams<'a> {
    pub st: &'a str,
    pub usn: &'a str,
    pub location: &'a str,
    pub server: &'a str,
    pub max_age: u32,
    pub boot_id: u32,
    pub config_id: u32,
}

pub fn build_response(p: &ResponseParams<'_>) -> String {
    let date = httpdate::fmt_http_date(std::time::SystemTime::now());
    format!(
        "HTTP/1.1 200 OK\r\n\
         CACHE-CONTROL: max-age={max_age}\r\n\
         DATE: {date}\r\n\
         EXT:\r\n\
         LOCATION: {location}\r\n\
         SERVER: {server}\r\n\
         ST: {st}\r\n\
         USN: {usn}\r\n\
         BOOTID.UPNP.ORG: {boot_id}\r\n\
         CONFIGID.UPNP.ORG: {config_id}\r\n\r\n",
        max_age = p.max_age,
        date = date,
        location = p.location,
        server = p.server,
        st = p.st,
        usn = p.usn,
        boot_id = p.boot_id,
        config_id = p.config_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MSEARCH: &str = "M-SEARCH * HTTP/1.1\r\n\
        HOST: 239.255.255.250:1900\r\n\
        MAN: \"ssdp:discover\"\r\n\
        MX: 3\r\n\
        ST: urn:schemas-upnp-org:device:MediaServer:1\r\n\
        USER-AGENT: vlc/3.0.21\r\n\r\n";

    #[test]
    fn parses_msearch() {
        let msg = Message::parse(MSEARCH).expect("parse");
        assert_eq!(msg.kind, MessageKind::Search);
        assert_eq!(msg.mx(), 3);
        assert_eq!(msg.search_target(), Some("urn:schemas-upnp-org:device:MediaServer:1"));
        assert_eq!(msg.header("user-agent"), Some("vlc/3.0.21"));
    }

    #[test]
    fn parses_notify_and_response() {
        let notify = "NOTIFY * HTTP/1.1\nHOST: 239.255.255.250:1900\nNT: upnp:rootdevice\nNTS: ssdp:alive\nUSN: uuid:x::upnp:rootdevice\n\n";
        let msg = Message::parse(notify).expect("parse notify");
        assert_eq!(msg.kind, MessageKind::Notify);
        assert!(!msg.is_byebye());
        assert_eq!(msg.search_target(), Some("upnp:rootdevice"));

        let byebye = Message::parse("NOTIFY * HTTP/1.1\r\nNTS: ssdp:byebye\r\nNT: upnp:rootdevice\r\n\r\n")
            .expect("parse byebye");
        assert!(byebye.is_byebye());

        let resp = Message::parse(
            "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nST: upnp:rootdevice\r\nLOCATION: http://10.0.0.10:8200/rootDesc.xml\r\n\r\n",
        )
        .expect("parse response");
        assert_eq!(resp.kind, MessageKind::Response);
        assert_eq!(resp.location(), Some("http://10.0.0.10:8200/rootDesc.xml"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(Message::parse("ovo nije ssdp").is_none());
        assert!(Message::parse("").is_none());
    }

    #[test]
    fn built_msearch_parses_back() {
        let raw = build_msearch("ssdp:all", 2);
        assert!(raw.starts_with("M-SEARCH * HTTP/1.1\r\n"));
        let msg = Message::parse(&raw).expect("parse own msearch");
        assert_eq!(msg.search_target(), Some("ssdp:all"));
        assert_eq!(msg.mx(), 2);
        assert!(raw.ends_with("\r\n\r\n"));
    }

    #[test]
    fn built_notify_has_required_headers() {
        let raw = build_notify(&NotifyParams {
            nts: "ssdp:alive",
            nt: "upnp:rootdevice",
            usn: "uuid:a::upnp:rootdevice",
            location: "http://10.0.0.1:8200/rootDesc.xml",
            server: "test/1.0 UPnP/1.0 Rustiio/0.1",
            max_age: 1800,
            boot_id: 7,
            config_id: 1,
        });
        let msg = Message::parse(&raw).expect("parse own notify");
        assert_eq!(msg.kind, MessageKind::Notify);
        assert_eq!(msg.header("cache-control"), Some("max-age=1800"));
        assert_eq!(msg.header("bootid.upnp.org"), Some("7"));
        assert!(msg.header("server").is_some());
    }

    #[test]
    fn built_response_has_ext_and_date() {
        let raw = build_response(&ResponseParams {
            st: "upnp:rootdevice",
            usn: "uuid:a::upnp:rootdevice",
            location: "http://10.0.0.1:8200/rootDesc.xml",
            server: "test/1.0 UPnP/1.0 Rustiio/0.1",
            max_age: 1800,
            boot_id: 7,
            config_id: 1,
        });
        let msg = Message::parse(&raw).expect("parse own response");
        assert_eq!(msg.kind, MessageKind::Response);
        assert_eq!(msg.header("ext"), Some(""));
        assert!(msg.header("date").is_some());
    }

    #[test]
    fn mx_defaults_and_clamps() {
        let msg = Message::parse("M-SEARCH * HTTP/1.1\r\nST: ssdp:all\r\n\r\n").expect("parse");
        assert_eq!(msg.mx(), 1);
        let msg2 = Message::parse("M-SEARCH * HTTP/1.1\r\nMX: 99\r\nST: ssdp:all\r\n\r\n").expect("parse");
        assert_eq!(msg2.mx(), 5);
    }
}
