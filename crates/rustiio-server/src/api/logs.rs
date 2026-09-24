//! Zadržane log linije + live tok za web sučelje.
//!
//! Dva kanala, ista linija:
//! - prstenasti buffer (zadnjih [`CAPACITY`]) → `GET /api/logs?limit=200`
//! - broadcast kanal → `GET /ws/logs` (WebSocket, live tail)
//!
//! Vrijeme se šalje kao `at_ms` (epoch), a UI ga formatira u lokalno — tako u
//! Rustu ne treba vremenska zona.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use axum::Router;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use serde::Serialize;
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

use crate::state::AppState;

/// Koliko linija pamtimo za `GET /api/logs`.
pub const CAPACITY: usize = 500;

/// Koliko linija može čekati u live toku (spor čitač preskače, ne blokira server).
const CHANNEL: usize = 512;

/// Jedna log linija u obliku koji UI razumije.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LogLine {
    /// Epoch milisekunde (UI formatira lokalno).
    pub at_ms: u64,
    /// `TRACE`/`DEBUG`/`INFO`/`WARN`/`ERROR`.
    pub level: String,
    /// Modul koji je pisao (`rustiio_library::metadata`).
    pub target: String,
    pub message: String,
}

impl LogLine {
    /// Tekst za kopiranje/„download" (bez boja) — npr. `INFO  rustiio_server  poruka`.
    pub fn text(&self) -> String {
        format!("{:<5} {}  {}", self.level, self.target, self.message)
    }
}

/// Prstenasti buffer + broadcast za pretplatnike.
pub struct LogBuffer {
    lines: Mutex<VecDeque<LogLine>>,
    events: broadcast::Sender<LogLine>,
}

impl LogBuffer {
    fn new() -> Self {
        let (events, _) = broadcast::channel(CHANNEL);
        Self { lines: Mutex::new(VecDeque::with_capacity(CAPACITY)), events }
    }

    /// Dodaj liniju (zove ga tracing sloj i `logs::note`).
    pub fn push(&self, line: LogLine) {
        if let Ok(mut lines) = self.lines.lock() {
            if lines.len() == CAPACITY {
                lines.pop_front();
            }
            lines.push_back(line.clone());
        }
        // Nitko ne sluša? Nije greška.
        let _ = self.events.send(line);
    }

    /// Zadnjih `limit` linija, najstarija prva.
    pub fn recent(&self, limit: usize) -> Vec<LogLine> {
        let Ok(lines) = self.lines.lock() else { return Vec::new() };
        let skip = lines.len().saturating_sub(limit);
        lines.iter().skip(skip).cloned().collect()
    }

    /// Pretplata na živi tok.
    pub fn subscribe(&self) -> broadcast::Receiver<LogLine> {
        self.events.subscribe()
    }

    /// Koliko linija trenutno imamo (za testove i `/api/status`).
    pub fn len(&self) -> usize {
        self.lines.lock().map(|lines| lines.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Globalni buffer (jedan po procesu, kao i logger).
pub fn global() -> &'static LogBuffer {
    static BUFFER: OnceLock<LogBuffer> = OnceLock::new();
    BUFFER.get_or_init(LogBuffer::new)
}

/// Upiši liniju iz koda koji nije `tracing` (npr. iz handlera).
pub fn note(level: &str, target: &str, message: impl Into<String>) {
    global().push(LogLine {
        at_ms: now_ms(),
        level: level.to_string(),
        target: target.to_string(),
        message: message.into(),
    });
}

/// Epoch milisekunde.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

// ------------------------------------------------------------------ tracing sloj

/// `tracing` sloj koji svaku liniju gura u [`global`] buffer.
///
/// Uključi ga u `init_tracing` (app crate) — bez toga UI nema što prikazati.
#[derive(Debug, Default, Clone, Copy)]
pub struct BufferLayer;

impl<S: Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        global().push(LogLine {
            at_ms: now_ms(),
            level: metadata.level().to_string(),
            target: metadata.target().to_string(),
            message: fields.0,
        });
    }
}

/// Skuplja polja događaja u jedan tekst (`poruka kljuc=value`).
struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            let text = format!("{value:?}");
            self.0 = text.trim_matches('"').to_string();
            return;
        }
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        self.0.push_str(&format!("{}={value:?}", field.name()));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
            return;
        }
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        self.0.push_str(&format!("{}={value}", field.name()));
    }
}

// ---------------------------------------------------------------------- rute

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/logs", get(api_logs)).route("/ws/logs", get(ws_logs))
}

/// `GET /api/logs?limit=200` — zadržane linije (najstarija prva).
async fn api_logs(axum::extract::Query(query): axum::extract::Query<LogsQuery>) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(200).min(CAPACITY);
    let lines = global().recent(limit);
    axum::Json(serde_json::json!({
        "lines": lines,
        "kept": global().len(),
        "capacity": CAPACITY,
    }))
}

#[derive(Debug, serde::Deserialize)]
struct LogsQuery {
    limit: Option<usize>,
}

/// `GET /ws/logs` — live tok: prvo zadržane linije, pa svaka nova.
async fn ws_logs(ws: axum::extract::WebSocketUpgrade, State(_state): State<AppState>) -> impl IntoResponse {
    let backlog = global().recent(100);
    let receiver = global().subscribe();
    ws.on_upgrade(move |socket| stream(socket, backlog, receiver))
}

async fn stream(
    mut socket: axum::extract::ws::WebSocket,
    backlog: Vec<LogLine>,
    mut receiver: broadcast::Receiver<LogLine>,
) {
    for line in backlog {
        if send_json(&mut socket, &line).await.is_err() {
            return;
        }
    }
    loop {
        match receiver.recv().await {
            Ok(line) => {
                if send_json(&mut socket, &line).await.is_err() {
                    return;
                }
            }
            // Spor čitač: preskoči propuštene linije i nastavi (UI ima /api/logs).
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

async fn send_json(socket: &mut axum::extract::ws::WebSocket, line: &LogLine) -> Result<(), axum::Error> {
    let text = serde_json::to_string(line).unwrap_or_else(|_| "{}".to_string());
    socket.send(axum::extract::ws::Message::Text(text.into())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(message: &str) -> LogLine {
        LogLine { at_ms: 1, level: "INFO".into(), target: "test".into(), message: message.into() }
    }

    #[test]
    fn buffer_keeps_only_capacity_lines() {
        let buffer = LogBuffer::new();
        for index in 0..(CAPACITY + 20) {
            buffer.push(line(&format!("linija {index}")));
        }
        assert_eq!(buffer.len(), CAPACITY);
        let recent = buffer.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[1].message, format!("linija {}", CAPACITY + 19));
        assert_eq!(recent[0].message, format!("linija {}", CAPACITY + 18));
    }

    #[test]
    fn recent_returns_oldest_first_and_respects_limit() {
        let buffer = LogBuffer::new();
        for index in 0..5 {
            buffer.push(line(&format!("l{index}")));
        }
        let recent = buffer.recent(3);
        assert_eq!(recent.iter().map(|l| l.message.as_str()).collect::<Vec<_>>(), ["l2", "l3", "l4"]);
    }

    #[test]
    fn subscribers_receive_new_lines() {
        let buffer = LogBuffer::new();
        let mut receiver = buffer.subscribe();
        buffer.push(line("zdravo"));
        assert_eq!(receiver.try_recv().expect("linija").message, "zdravo");
    }

    #[test]
    fn text_is_plain_for_copy_paste() {
        let log = LogLine {
            at_ms: 0,
            level: "WARN".into(),
            target: "rustiio_library::metadata".into(),
            message: "nema postera".into(),
        };
        assert_eq!(log.text(), "WARN  rustiio_library::metadata  nema postera");
    }
}
