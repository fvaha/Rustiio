//! HTTP rute: UPnP (device.xml, SCPD, control) + mediji + mali REST API.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{any, get, post};
use serde_json::json;
use tower_http::trace::TraceLayer;
use tracing::{debug, warn};

use rustiio_cds::{BrowseRequest, MAX_RESULTS, browse, sort_capabilities};
use rustiio_upnp::protocol::{self};
use rustiio_upnp::{device_description, escape, scpd, soap};

use crate::state::AppState;

const XML_CONTENT_TYPE: &str = "text/xml; charset=\"utf-8\"";

/// Sastavi router (bez bindanja — to radi `apps/rustiio`).
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(health))
        .route("/rootDesc.xml", get(root_desc))
        .route("/ContentDirectory/scpd.xml", get(content_directory_scpd))
        .route("/ConnectionManager/scpd.xml", get(connection_manager_scpd))
        .route("/ContentDirectory/control", post(content_directory_control))
        .route("/ConnectionManager/control", post(connection_manager_control))
        .route("/ContentDirectory/event", any(eventing))
        .route("/ConnectionManager/event", any(eventing))
        .route("/res/{id}", get(media_by_id))
        .route("/res/{id}/{filename}", get(media_by_name))
        .route("/sub/{id}/{filename}", get(subtitle_by_name))
        .route("/api/status", get(api_status))
        .route("/api/rescan", post(api_rescan))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

// ---------------------------------------------------------------- UPnP osnova

async fn root_desc(State(state): State<AppState>) -> Response {
    xml_response(device_description(&state.device_meta()))
}

async fn content_directory_scpd() -> Response {
    static_response(scpd::CONTENT_DIRECTORY_SCPD)
}

async fn connection_manager_scpd() -> Response {
    static_response(scpd::CONNECTION_MANAGER_SCPD)
}

/// Minimalni eventing: klijent dobije `SID` i `TIMEOUT`, ali dogadjaje jos ne saljemo.
///
/// TV-i (Samsung) znaju odustati od servisa ako `SUBSCRIBE` vrati gresku, zato
/// radije pristojno potvrdimo pretplatu. Pravi NOTIFY dolazi u Fazi 6.
async fn eventing(headers: HeaderMap) -> Response {
    let is_unsubscribe = headers.get("sid").is_some() && headers.get("callback").is_none();
    let sid = headers
        .get("sid")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("uuid:{}", uuid_like()));

    let mut builder = Response::builder().status(StatusCode::OK);
    if !is_unsubscribe {
        builder = builder.header("SID", sid).header("TIMEOUT", "Second-1800");
    }
    builder
        .body(axum::body::Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

// ------------------------------------------------------------ SOAP / control

async fn content_directory_control(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let action_header = header_str(&headers, "soapaction").unwrap_or_default();
    let Some(request) = soap::parse_action_header(&action_header) else {
        warn!(header = %action_header, "SOAP zahtjev bez valjanog SOAPACTION headera");
        return soap_fault_response(401, "Invalid Action");
    };
    let args = soap::parse_args(&String::from_utf8_lossy(&body));

    debug!(
        action = %request.action,
        device = %header_str(&headers, "user-agent").unwrap_or_else(|| "-".to_string()),
        "ContentDirectory akcija"
    );

    match request.action.as_str() {
        "Browse" => {
            let browse_request = BrowseRequest {
                object_id: arg(&args, "ObjectID").unwrap_or_else(|| "0".to_string()),
                browse_flag: arg(&args, "BrowseFlag").unwrap_or_default(),
                filter: arg(&args, "Filter").unwrap_or_else(|| "*".to_string()),
                starting_index: arg(&args, "StartingIndex").and_then(|v| v.parse().ok()).unwrap_or(0),
                requested_count: arg(&args, "RequestedCount").and_then(|v| v.parse().ok()).unwrap_or(0),
                sort_criteria: arg(&args, "SortCriteria").unwrap_or_default(),
            };
            let catalog = state.catalog.read().await;
            match browse(&catalog, &browse_request, &state.base_url, MAX_RESULTS) {
                Ok(outcome) => {
                    debug!(
                        object_id = %browse_request.object_id,
                        total = outcome.total,
                        returned = outcome.returned,
                        "Browse"
                    );
                    let inner = format!(
                        "      <Result>{}</Result>\n      <NumberReturned>{}</NumberReturned>\n      <TotalMatches>{}</TotalMatches>\n      <UpdateID>{}</UpdateID>",
                        escape(&outcome.didl),
                        outcome.returned,
                        outcome.total,
                        outcome.update_id
                    );
                    xml_response(soap::response(&request.service, &request.action, &inner))
                }
                Err(err) => soap_fault_response(err.code(), &err.description()),
            }
        }
        // Prava pretraga dolazi s FTS indeksom u Fazi 3; prazan rezultat je valjan odgovor.
        "Search" => {
            let catalog = state.catalog.read().await;
            let inner = format!(
                "      <Result>{}</Result>\n      <NumberReturned>0</NumberReturned>\n      <TotalMatches>0</TotalMatches>\n      <UpdateID>{}</UpdateID>",
                escape(&rustiio_upnp::render_didl(&[])),
                catalog.update_id
            );
            xml_response(soap::response(&request.service, &request.action, &inner))
        }
        "GetSortCapabilities" => xml_response(soap::response(
            &request.service,
            &request.action,
            &format!("      <SortCaps>{}</SortCaps>", sort_capabilities()),
        )),
        "GetSearchCapabilities" => xml_response(soap::response(
            &request.service,
            &request.action,
            "      <SearchCaps>dc:title</SearchCaps>",
        )),
        "GetSystemUpdateID" => {
            let catalog = state.catalog.read().await;
            xml_response(soap::response(
                &request.service,
                &request.action,
                &format!("      <Id>{}</Id>", catalog.update_id),
            ))
        }
        other => {
            warn!(action = %other, "nepodrzana ContentDirectory akcija");
            soap_fault_response(401, "Invalid Action")
        }
    }
}

async fn connection_manager_control(
    State(state): State<AppState>,
    headers: HeaderMap,
    _body: Bytes,
) -> Response {
    let action_header = header_str(&headers, "soapaction").unwrap_or_default();
    let Some(request) = soap::parse_action_header(&action_header) else {
        return soap_fault_response(401, "Invalid Action");
    };

    match request.action.as_str() {
        "GetProtocolInfo" => {
            let source = protocol::source_protocol_info(&state.config.library.video_extensions);
            let inner = format!("      <Source>{}</Source>\n      <Sink></Sink>", escape(&source));
            xml_response(soap::response(&request.service, &request.action, &inner))
        }
        "GetCurrentConnectionIDs" => xml_response(soap::response(
            &request.service,
            &request.action,
            "      <ConnectionIDs>0</ConnectionIDs>",
        )),
        "GetCurrentConnectionInfo" => xml_response(soap::response(
            &request.service,
            &request.action,
            "      <RcsID>0</RcsID>\n      <AVTransportID>0</AVTransportID>\n      <ProtocolInfo></ProtocolInfo>\n      <PeerConnectionManager></PeerConnectionManager>\n      <PeerConnectionID>-1</PeerConnectionID>\n      <Direction>Output</Direction>\n      <Status>OK</Status>",
        )),
        other => {
            warn!(action = %other, "nepodrzana ConnectionManager akcija");
            soap_fault_response(401, "Invalid Action")
        }
    }
}

// -------------------------------------------------------------------- mediji

async fn media_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    serve_node(&state, &id, &headers, method == Method::HEAD).await
}

async fn media_by_name(
    State(state): State<AppState>,
    Path((id, _filename)): Path<(String, String)>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    serve_node(&state, &id, &headers, method == Method::HEAD).await
}

async fn subtitle_by_name(
    State(state): State<AppState>,
    Path((id, _filename)): Path<(String, String)>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    let path = {
        let catalog = state.catalog.read().await;
        catalog.get(&id).and_then(|node| node.subtitle.clone())
    };
    match path {
        Some(path) => rustiio_http::serve_file(&path, &headers, method == Method::HEAD).await,
        None => (StatusCode::NOT_FOUND, "no subtitle").into_response(),
    }
}

async fn serve_node(state: &AppState, id: &str, headers: &HeaderMap, head_only: bool) -> Response {
    let path = {
        let catalog = state.catalog.read().await;
        match catalog.get(id) {
            Some(node) if !node.is_container() => Some(node.path.clone()),
            _ => None,
        }
    };
    match path {
        Some(path) => rustiio_http::serve_file(&path, headers, head_only).await,
        None => (StatusCode::NOT_FOUND, "no such object").into_response(),
    }
}

// --------------------------------------------------------------- REST i web

async fn api_status(State(state): State<AppState>) -> Response {
    let catalog = state.catalog.read().await;
    let counts = catalog.counts();
    let roots: Vec<_> = state
        .config
        .library
        .roots
        .iter()
        .map(|root| {
            json!({
                "label": root.label,
                "path": root.path.display().to_string(),
                "exists": root.path.is_dir(),
            })
        })
        .collect();

    axum::Json(json!({
        "app": rustiio_core::APP_NAME,
        "version": rustiio_core::VERSION,
        "udn": state.identity.udn,
        "friendly_name": state.identity.friendly_name,
        "base_url": state.base_url.as_str(),
        "http_port": state.config.server.http_port,
        "ssdp": state.config.server.ssdp,
        "transcode_enabled": state.config.transcode.enabled,
        "uptime_secs": state.uptime_secs(),
        "update_id": catalog.update_id,
        "items": catalog.len(),
        "counts": counts,
        "roots": roots,
    }))
    .into_response()
}

async fn api_rescan(State(state): State<AppState>) -> Response {
    let count = state.rescan().await;
    let update_id = state.catalog.read().await.update_id;
    axum::Json(json!({ "ok": true, "items": count, "update_id": update_id })).into_response()
}

async fn health(State(state): State<AppState>) -> Response {
    let catalog = state.catalog.read().await;
    axum::Json(json!({
        "ok": true,
        "app": rustiio_core::APP_NAME,
        "version": rustiio_core::VERSION,
        "uptime_secs": state.uptime_secs(),
        "items": catalog.len(),
    }))
    .into_response()
}

async fn index(State(state): State<AppState>) -> Response {
    let catalog = state.catalog.read().await;
    let page = format!(
        r#"<!doctype html>
<html lang="hr"><head><meta charset="utf-8"><title>{app}</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
 :root {{ color-scheme: dark; }}
 body {{ margin:0; font:15px/1.6 ui-sans-serif,system-ui,-apple-system,"Segoe UI",sans-serif;
        background:#0b0d10; color:#e6e8eb; }}
 .wrap {{ max-width: 720px; margin: 0 auto; padding: 56px 24px; }}
 h1 {{ margin:0 0 4px; font-size:26px; letter-spacing:-.02em; }}
 .muted {{ color:#8b949e; }}
 .card {{ background:#12151a; border:1px solid #1f242b; border-radius:14px; padding:18px 20px; margin-top:22px; }}
 a {{ color:#7cc4ff; text-decoration:none; }} a:hover {{ text-decoration:underline; }}
 code {{ background:#1a1f26; padding:2px 6px; border-radius:6px; }}
 .grid {{ display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:12px; margin-top:14px; }}
 .kpi {{ background:#171b21; border:1px solid #232932; border-radius:10px; padding:12px 14px; }}
 .kpi b {{ display:block; font-size:22px; }}
</style></head><body><div class="wrap">
 <h1>{app} <span class="muted">v{version}</span></h1>
 <div class="muted">{friendly} — DLNA/UPnP media server radi.</div>
 <div class="grid">
   <div class="kpi"><b>{items}</b><span class="muted">objekata u biblioteci</span></div>
   <div class="kpi"><b>{uptime}</b><span class="muted">sekundi rada</span></div>
   <div class="kpi"><b>{roots}</b><span class="muted">mapa</span></div>
 </div>
 <div class="card">
   <b>Na TV-u:</b> otvori izvor <code>{friendly}</code> u DLNA/UPnP izborniku.<br>
   <b>U VLC-u:</b> Local Network → Universal Plug'n'Play.<br>
   <b>API:</b> <a href="/healthz">/healthz</a> ·
   <a href="/api/status">/api/status</a> ·
   <a href="/rootDesc.xml">/rootDesc.xml</a>
 </div>
 <div class="card muted">
   Web sučelje (dashboard, biblioteka, profili) dolazi u Fazi 4.
 </div>
</div></body></html>"#,
        app = rustiio_core::APP_NAME,
        version = rustiio_core::VERSION,
        friendly = escape(&state.identity.friendly_name),
        items = catalog.len(),
        uptime = state.uptime_secs(),
        roots = state.config.library.roots.len(),
    );
    Html(page).into_response()
}

// ------------------------------------------------------------------- pomocno

fn xml_response(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, XML_CONTENT_TYPE)
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn static_response(body: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, XML_CONTENT_TYPE)
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// SOAP fault ide s HTTP 500, kako UPnP spec trazi.
fn soap_fault_response(code: u32, description: &str) -> Response {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, XML_CONTENT_TYPE)
        .header("ext", "")
        .body(axum::body::Body::from(soap::fault(code, description)))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name).and_then(|value| value.to_str().ok()).map(|value| value.to_string())
}

fn arg(args: &std::collections::HashMap<String, String>, name: &str) -> Option<String> {
    args.get(name).cloned()
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{nanos:032x}-{:04x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustiio_core::DeviceIdentity;
    use rustiio_library::{Catalog, ScanOptions};

    fn state() -> AppState {
        let mut config = rustiio_core::Config::default();
        config.server.udn = Some("uuid:test".to_string());
        config.server.friendly_name = Some("Rustiio (test)".to_string());
        let identity = DeviceIdentity::ensure(&mut config, std::path::Path::new("")).unwrap();
        AppState::new(
            std::sync::Arc::new(config),
            identity,
            "http://127.0.0.1:8200".to_string(),
            Catalog::default(),
            ScanOptions::default(),
        )
    }

    #[test]
    fn router_builds_without_panic() {
        let _ = router(state());
    }

    #[test]
    fn uuid_like_is_unique_enough() {
        assert_ne!(uuid_like(), uuid_like());
    }

    #[test]
    fn mime_helper_is_reachable() {
        assert_eq!(rustiio_upnp::protocol::mime_for_ext("mkv"), "video/x-matroska");
    }
}
