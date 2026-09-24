//! `/api/browse` — isti sadržaj koji TV vidi, ali u JSON-u za web sučelje.
//!
//! Web sučelje **ne** parsira DIDL: ovo je zasebna projekcija kataloga (id, naslov,
//! vrsta, veličina, poster, izravni link). Poster dolazi iz istog izvora kao za DIDL
//! ([`StoreArt`]), pa browser i TV nikad ne tvrde različito.
//!
//! Uz to: `/api/export` (CSV/JSON popis cijele biblioteke) i `/api/items/{id}`.

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use rustiio_cds::ArtLookup;
use rustiio_library::Node;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::art::StoreArt;
use crate::state::AppState;

/// Koliko objekata najviše vrati jedan poziv (web traži stranicu po stranicu).
const MAX_LIMIT: usize = 500;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/browse", get(api_browse))
        .route("/api/items/{id}", get(api_item))
        .route("/api/export", get(api_export))
}

#[derive(Debug, Deserialize)]
struct BrowseQuery {
    /// UPnP id mape (`0` = korijen).
    id: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
    /// Filtar naslova (bez razlike velikih/malih slova).
    q: Option<String>,
    /// `video`/`audio`/`image`/`folder`.
    kind: Option<String>,
}

/// `GET /api/browse?id=0&limit=200&q=&kind=`
async fn api_browse(State(state): State<AppState>, Query(query): Query<BrowseQuery>) -> Response {
    let catalog = state.catalog.read().await;
    let id = query.id.unwrap_or_else(|| "0".to_string());
    let Some(node) = catalog.get(&id) else {
        return fail(StatusCode::NOT_FOUND, format!("nema objekta {id}"));
    };
    let art = StoreArt::new(state.store.clone(), state.base_url.clone());

    let mut items: Vec<(bool, String, Value)> = catalog
        .children(&id)
        .iter()
        .filter(|child| match &query.kind {
            Some(kind) => kind_name(child.kind) == kind.to_ascii_lowercase(),
            None => true,
        })
        .filter(|child| match &query.q {
            Some(text) if !text.trim().is_empty() => {
                child.title.to_lowercase().contains(&text.trim().to_lowercase())
            }
            _ => true,
        })
        .map(|child| {
            // Mape prve, pa po naslovu — kao što TV dobiva.
            (child.is_container(), child.title.to_lowercase(), project(child, &art, &state))
        })
        .collect();
    items.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));

    let total = items.len();
    let offset = query.offset.unwrap_or(0).min(total);
    let limit = query.limit.unwrap_or(MAX_LIMIT).min(MAX_LIMIT);
    let page: Vec<Value> = items.into_iter().skip(offset).take(limit).map(|(_, _, item)| item).collect();

    axum::Json(json!({
        "id": node.id,
        "title": node.title,
        "parent": node.parent_id,
        "container": node.is_container(),
        "total": total,
        "offset": offset,
        "limit": limit,
        "items": page,
    }))
    .into_response()
}

/// `GET /api/items/{id}` — jedan objekt (za detalje i "pusti").
async fn api_item(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let catalog = state.catalog.read().await;
    let Some(node) = catalog.get(&id) else {
        return fail(StatusCode::NOT_FOUND, format!("nema objekta {id}"));
    };
    let art = StoreArt::new(state.store.clone(), state.base_url.clone());
    axum::Json(json!({
        "item": project(node, &art, &state),
        "putanja": node.path.display().to_string(),
        "djeca": node.children.len(),
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
struct ExportQuery {
    /// `csv` (zadano) ili `json`.
    format: Option<String>,
}

/// `GET /api/export?format=csv|json` — cijela biblioteka u jednoj datoteci.
///
/// Namjerno jednostavno: ravne kolone koje tablica/urednik mogu pročitati.
async fn api_export(State(state): State<AppState>, Query(query): Query<ExportQuery>) -> Response {
    let catalog = state.catalog.read().await;
    let art = StoreArt::new(state.store.clone(), state.base_url.clone());
    let rows: Vec<Node> = catalog.nodes().filter(|node| !node.is_container()).cloned().collect();
    let items: Vec<Value> = rows.iter().map(|node| project(node, &art, &state)).collect();

    match query.format.as_deref().unwrap_or("csv") {
        "json" => axum::Json(json!({ "items": items, "count": items.len() })).into_response(),
        _ => {
            let mut csv = String::from("id,naslov,vrsta,godina,velicina_b,putanja,poster,link\n");
            for (node, item) in rows.iter().zip(&items) {
                let title = csv_field(&node.title);
                let path = csv_field(&node.path.display().to_string());
                let poster = item.get("poster").and_then(Value::as_str).unwrap_or("");
                let play = item.get("play").and_then(Value::as_str).unwrap_or("");
                let year = item.get("year").map(|year| year.to_string()).unwrap_or_default();
                csv.push_str(&format!(
                    "{},{title},{},{year},{},{path},{poster},{play}\n",
                    node.id,
                    kind_name(node.kind),
                    node.size
                ));
            }
            (
                [
                    (axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8"),
                    (
                        axum::http::header::CONTENT_DISPOSITION,
                        "attachment; filename=\"rustiio-biblioteka.csv\"",
                    ),
                ],
                csv,
            )
                .into_response()
        }
    }
}

/// Jedan objekt u obliku koji web sučelje koristi.
fn project(node: &Node, art: &dyn ArtLookup, state: &AppState) -> Value {
    let container = node.is_container();
    let path = node.path.display().to_string();
    json!({
        "id": node.id,
        "title": node.title,
        "kind": kind_name(node.kind),
        "container": container,
        "children": node.children.len(),
        "size": node.size,
        "modified_ms": node.modified.and_then(|time| {
            time.duration_since(std::time::UNIX_EPOCH).ok().map(|since| since.as_millis())
        }),
        "subtitle": node.subtitle.as_ref().map(|sub| sub.file_name().unwrap_or_default().to_string_lossy().to_string()),
        "poster": art.art_url(&node.id),
        "play": (!container).then(|| {
            let name = urlencode(&node.file_name());
            format!("{}/res/{}/{name}", state.base_url, node.id)
        }),
        "putanja": path,
    })
}

fn kind_name(kind: rustiio_library::NodeKind) -> String {
    use rustiio_library::NodeKind;
    match kind {
        NodeKind::Container => "folder",
        NodeKind::Video => "video",
        NodeKind::Audio => "audio",
        NodeKind::Image => "image",
        // Titlovi i ostalo se u sučelju ne prikazuju odvojeno — idu uz video.
        NodeKind::Subtitle => "subtitle",
        NodeKind::Other => "other",
    }
    .to_string()
}

/// Naslov/putanja za CSV: navodnici oko svega što ima zarez ili navodnik.
fn csv_field(text: &str) -> String {
    if text.contains(',') || text.contains('"') || text.contains('\n') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// Minimalno kodiranje za ime datoteke u URL-u (razmak, dijakritika, `#`).
fn urlencode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn fail(status: StatusCode, message: impl Into<String>) -> Response {
    (status, axum::Json(json!({ "greska": message.into() }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_field_quotes_separators() {
        assert_eq!(csv_field("Sicario"), "Sicario");
        assert_eq!(csv_field("Zestoki, decki"), "\"Zestoki, decki\"");
        assert_eq!(csv_field("On je rekao \"zdravo\""), "\"On je rekao \"\"zdravo\"\"\"");
    }

    #[test]
    fn urlencode_escapes_spaces_and_unicode() {
        assert_eq!(urlencode("Test Film.mkv"), "Test%20Film.mkv");
        assert_eq!(urlencode("Šuma.mp4"), "%C5%A0uma.mp4");
    }
}
