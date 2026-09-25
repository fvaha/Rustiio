//! GENA — UPnP eventing: `SUBSCRIBE` / `UNSUBSCRIBE` + `NOTIFY` prema klijentu.
//!
//! Bez ovoga TV nema pojma da se biblioteka promijenila i drzi stari popis (Serviio
//! salje prave evente; lazni SID bez ijednog NOTIFY-a je cest izvor "TV ne vidi novi
//! film"). Ovdje je drzava pretplata u memoriji — dovoljno, jer se TV sam pretplati
//! nakon svakog ukljucenja, a pretplata ionako istjece.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, warn};

/// Koliko dugo pretplata vrijedi (TV obicno trazi 1800 s).
pub const TIMEOUT_SECS: u64 = 1800;
/// Gornja granica koju dopustamo klijentu.
const MAX_TIMEOUT_SECS: u64 = 3600;

pub const CONTENT_DIRECTORY: &str = "ContentDirectory";
pub const CONNECTION_MANAGER: &str = "ConnectionManager";

#[derive(Debug, Clone)]
pub struct Subscription {
    pub sid: String,
    pub service: String,
    pub callbacks: Vec<String>,
    pub expires_at: Instant,
    pub seq: u32,
}

impl Subscription {
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    pub fn expires_in(&self) -> u64 {
        self.expires_at.saturating_duration_since(Instant::now()).as_secs()
    }
}

/// Registar pretplata (jedan po serveru).
#[derive(Default)]
pub struct Registry {
    subscriptions: Mutex<HashMap<String, Subscription>>,
    counter: AtomicU32,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Nova pretplata; SID mora biti jedinstven unutar servera.
    pub fn subscribe(&self, service: &str, callbacks: Vec<String>) -> Subscription {
        let number = self.counter.fetch_add(1, Ordering::Relaxed);
        let sid = format!("uuid:rustiio-{:x}-{:x}", unique_stamp(), number);
        let subscription = Subscription {
            sid: sid.clone(),
            service: service.to_string(),
            callbacks,
            expires_at: Instant::now() + Duration::from_secs(TIMEOUT_SECS),
            seq: 0,
        };
        if let Ok(mut map) = self.subscriptions.lock() {
            map.insert(sid, subscription.clone());
        }
        subscription
    }

    /// Obnova postojece pretplate (TV salje `SUBSCRIBE` samo sa `SID`).
    pub fn renew(&self, sid: &str) -> Option<Subscription> {
        let mut map = self.subscriptions.lock().ok()?;
        let subscription = map.get_mut(sid)?;
        subscription.expires_at = Instant::now() + Duration::from_secs(TIMEOUT_SECS);
        Some(subscription.clone())
    }

    pub fn unsubscribe(&self, sid: &str) -> bool {
        self.subscriptions.lock().map(|mut map| map.remove(sid).is_some()).unwrap_or(false)
    }

    pub fn list(&self, service: &str) -> Vec<Subscription> {
        let Ok(map) = self.subscriptions.lock() else { return Vec::new() };
        map.values().filter(|s| s.service == service && !s.is_expired()).cloned().collect()
    }

    pub fn bump_seq(&self, sid: &str) {
        if let Ok(mut map) = self.subscriptions.lock() {
            if let Some(subscription) = map.get_mut(sid) {
                subscription.seq = subscription.seq.wrapping_add(1);
            }
        }
    }

    /// Ocisti istekle pretplate; vraca koliko ih je bilo.
    pub fn purge_expired(&self) -> usize {
        let Ok(mut map) = self.subscriptions.lock() else { return 0 };
        let before = map.len();
        map.retain(|_, subscription| !subscription.is_expired());
        before - map.len()
    }

    pub fn len(&self) -> usize {
        self.subscriptions.lock().map(|map| map.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[cfg(test)]
    fn expire_all(&self) {
        if let Ok(mut map) = self.subscriptions.lock() {
            for subscription in map.values_mut() {
                subscription.expires_at = Instant::now() - Duration::from_secs(1);
            }
        }
    }
}

fn unique_stamp() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let previous = LAST.swap(nanos, Ordering::Relaxed);
    if nanos <= previous { previous + 1 } else { nanos }
}

/// `CALLBACK` header: `<http://10.0.0.100:55000/ev> <http://10.0.0.5/ev>`.
pub fn parse_callbacks(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in value.split('<') {
        if let Some((url, _)) = chunk.split_once('>') {
            let url = url.trim();
            if !url.is_empty() {
                out.push(url.to_string());
            }
        }
    }
    if out.is_empty() {
        // Klijent nije stavio zagrade — pokusaj kao golu listu.
        for candidate in value.split_whitespace() {
            let candidate = candidate.trim_matches(|c| c == '<' || c == '>');
            if candidate.starts_with("http://") {
                out.push(candidate.to_string());
            }
        }
    }
    out
}

/// `http://10.0.0.100:55000/upnp/event` -> `("10.0.0.100:55000", "/upnp/event")`.
pub fn parse_callback_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.split_once('/') {
        Some((authority, tail)) => (authority, format!("/{tail}")),
        None => (rest, "/".to_string()),
    };
    let authority = authority.trim();
    if authority.is_empty() {
        return None;
    }
    let host_header = if authority.contains(':') { authority.to_string() } else { format!("{authority}:80") };
    Some((host_header, path))
}

/// TV-u kazemo dokad vrijedi pretplata (postujemo njegov zahtjev do 1 h).
pub fn timeout_header(requested: Option<&str>) -> String {
    let seconds = requested
        .and_then(|value| value.trim().strip_prefix("Second-"))
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(TIMEOUT_SECS)
        .min(MAX_TIMEOUT_SECS);
    format!("Second-{seconds}")
}

/// Posalji `NOTIFY` na prvi callback koji prihvati; `true` ako je barem jedan prosao.
pub async fn send_notify(subscription: &Subscription, body: &str, timeout: Duration) -> bool {
    for callback in &subscription.callbacks {
        let Some((host, path)) = parse_callback_url(callback) else {
            warn!(callback = %callback, "callback URL nije upotrebljiv");
            continue;
        };
        match notify_once(&host, &path, &subscription.sid, subscription.seq, body, timeout).await {
            Ok(()) => {
                debug!(sid = %subscription.sid, seq = subscription.seq, callback = %callback, "NOTIFY poslan");
                return true;
            }
            Err(err) => {
                warn!(sid = %subscription.sid, callback = %callback, error = %err, "NOTIFY nije prosao");
            }
        }
    }
    false
}

async fn notify_once(
    host: &str,
    path: &str,
    sid: &str,
    seq: u32,
    body: &str,
    timeout: Duration,
) -> std::io::Result<()> {
    let request = format!(
        "NOTIFY {path} HTTP/1.1\r\n\
         HOST: {host}\r\n\
         CONTENT-TYPE: text/xml; charset=\"utf-8\"\r\n\
         CONTENT-LENGTH: {length}\r\n\
         NT: upnp:event\r\n\
         NTS: upnp:propchange\r\n\
         SID: {sid}\r\n\
         SEQ: {seq}\r\n\
         CONNECTION: close\r\n\
         \r\n\
         {body}",
        length = body.len()
    );

    let mut stream = tokio::time::timeout(timeout, TcpStream::connect(host)).await??;
    tokio::time::timeout(timeout, stream.write_all(request.as_bytes())).await??;
    tokio::time::timeout(timeout, stream.flush()).await??;

    // Klijent mora odgovoriti 200 OK; citamo samo prvu liniju (best effort).
    let mut buffer = [0u8; 128];
    match tokio::time::timeout(timeout, stream.read(&mut buffer)).await {
        Ok(Ok(read)) if read > 0 => {
            let status = String::from_utf8_lossy(&buffer[..read]).lines().next().unwrap_or("").to_string();
            if status.contains("200") {
                Ok(())
            } else {
                Err(std::io::Error::other(format!("klijent vratio: {status}")))
            }
        }
        Ok(Ok(_)) => Err(std::io::Error::other("klijent zatvorio vezu bez odgovora")),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "nema odgovora na NOTIFY")),
    }
}

/// Tijelo eventa za ContentDirectory (`SystemUpdateID` je jedina eventirana varijabla).
pub fn content_directory_body(update_id: u32) -> String {
    propertyset(&[("SystemUpdateID", update_id.to_string())])
}

/// Tijelo eventa za ConnectionManager.
pub fn connection_manager_body(protocol_info: &str, connection_ids: &str) -> String {
    propertyset(&[
        ("SourceProtocolInfo", protocol_info.to_string()),
        ("CurrentConnectionIDs", connection_ids.to_string()),
    ])
}

fn propertyset(properties: &[(&str, String)]) -> String {
    let mut body = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <e:propertyset xmlns:e=\"urn:schemas-upnp-org:event-1-0\">\n",
    );
    for (name, value) in properties {
        body.push_str(&format!("  <e:property><{name}>{}</{name}></e:property>\n", escape_xml(value)));
    }
    body.push_str("</e:propertyset>\n");
    body
}

fn escape_xml(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn parses_callback_list() {
        let callbacks = parse_callbacks("<http://10.0.0.100:55000/ev> <http://10.0.0.5/ev2>");
        assert_eq!(callbacks, vec!["http://10.0.0.100:55000/ev", "http://10.0.0.5/ev2"]);
        assert_eq!(parse_callbacks("<http://1.2.3.4/only>"), vec!["http://1.2.3.4/only"]);
        assert!(parse_callbacks("").is_empty());
    }

    #[test]
    fn parses_callback_url_with_default_port() {
        assert_eq!(
            parse_callback_url("http://10.0.0.5:1234/upnp/event"),
            Some(("10.0.0.5:1234".to_string(), "/upnp/event".to_string()))
        );
        assert_eq!(parse_callback_url("http://10.0.0.5"), Some(("10.0.0.5:80".to_string(), "/".to_string())));
        assert_eq!(parse_callback_url("https://10.0.0.5/ev"), None, "samo http");
        assert_eq!(parse_callback_url("http:///nema"), None);
    }

    #[test]
    fn timeout_is_echoed_but_clamped() {
        assert_eq!(timeout_header(Some("Second-300")), "Second-300");
        assert_eq!(timeout_header(Some("Second-99999")), "Second-3600");
        assert_eq!(timeout_header(Some("infinite")), "Second-1800");
        assert_eq!(timeout_header(None), "Second-1800");
    }

    #[test]
    fn registry_lifecycle() {
        let registry = Registry::new();
        let subscription = registry.subscribe(CONTENT_DIRECTORY, vec!["http://10.0.0.5:9/ev".into()]);
        assert!(subscription.sid.starts_with("uuid:rustiio-"));
        assert_eq!(registry.list(CONTENT_DIRECTORY).len(), 1);
        assert!(registry.list(CONNECTION_MANAGER).is_empty());

        assert!(registry.renew("nepostoji").is_none());
        let renewed = registry.renew(&subscription.sid).expect("obnova");
        assert_eq!(renewed.sid, subscription.sid);
        assert_eq!(renewed.seq, 0);

        registry.bump_seq(&subscription.sid);
        assert_eq!(registry.list(CONTENT_DIRECTORY)[0].seq, 1);

        assert!(registry.unsubscribe(&subscription.sid));
        assert!(!registry.unsubscribe(&subscription.sid));
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn expired_subscriptions_are_purged() {
        let registry = Registry::new();
        registry.subscribe(CONTENT_DIRECTORY, vec!["http://10.0.0.5:9/ev".into()]);
        assert_eq!(registry.purge_expired(), 0);
        registry.expire_all();
        assert!(registry.list(CONTENT_DIRECTORY).is_empty(), "istekla se ne prikazuje");
        assert_eq!(registry.purge_expired(), 1);
        assert!(registry.is_empty());
    }

    #[test]
    fn event_bodies_carry_the_variables() {
        let body = content_directory_body(7);
        assert!(body.contains("<SystemUpdateID>7</SystemUpdateID>"));
        assert!(body.contains("urn:schemas-upnp-org:event-1-0"));
        assert!(body.contains("e:propertyset"));

        let cm = connection_manager_body("http-get:*:video/mp4:*", "");
        assert!(cm.contains("SourceProtocolInfo"));
        assert!(cm.contains("CurrentConnectionIDs"));
    }

    #[test]
    fn event_body_escapes_xml() {
        let body = connection_manager_body("http-get:*:a&b:*", "<1>");
        assert!(body.contains("a&amp;b"), "ampersand mora biti escapovan: {body}");
        assert!(body.contains("&lt;1&gt;"));
    }

    #[tokio::test]
    async fn notify_reaches_a_real_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let registry = Registry::new();
        let subscription = registry.subscribe(CONTENT_DIRECTORY, vec![format!("http://{address}/ev")]);

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0u8; 2048];
            let read = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            socket.write_all(b"HTTP/1.1 200 OK\r\nCONTENT-LENGTH: 0\r\n\r\n").await.unwrap();
            request
        });

        let body = content_directory_body(42);
        assert!(send_notify(&subscription, &body, Duration::from_secs(3)).await);

        let request = server.await.unwrap();
        assert!(request.starts_with("NOTIFY /ev HTTP/1.1"), "{request}");
        assert!(request.contains("NT: upnp:event"), "{request}");
        assert!(request.contains("NTS: upnp:propchange"), "{request}");
        assert!(request.contains(&format!("SID: {}", subscription.sid)), "{request}");
        assert!(request.contains("SEQ: 0"), "{request}");
        assert!(request.contains("<SystemUpdateID>42</SystemUpdateID>"), "{request}");
    }

    #[tokio::test]
    async fn notify_to_dead_callback_reports_failure() {
        // Port 9 (discard) na localhostu gotovo sigurno ne slusa.
        let registry = Registry::new();
        let subscription = registry.subscribe(CONTENT_DIRECTORY, vec!["http://127.0.0.1:9/ev".into()]);
        assert!(!send_notify(&subscription, &content_directory_body(1), Duration::from_millis(300)).await);
    }
}
