//! HTTP rute: UPnP (device.xml, SCPD, control, eventing) + mediji + REST API.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use serde_json::json;
use tokio_util::io::ReaderStream;
use tower_http::trace::TraceLayer;
use tracing::{debug, info, warn};

use rustiio_cds::{BrowseOptions, BrowseRequest, MAX_RESULTS, browse, sort_capabilities};
use rustiio_profiles::{DeviceIdentity as DeviceKey, Profile};
use rustiio_transcode::{PlaybackMode, StartRequest};
use rustiio_upnp::protocol;
use rustiio_upnp::{device_description, escape, scpd, soap};

use crate::gena;
use crate::playback::PlaybackEngine;
use crate::state::AppState;

const XML_CONTENT_TYPE: &str = "text/xml; charset=\"utf-8\"";

/// Sastavi router (bez bindanja — to radi `apps/rustiio`).
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/rootDesc.xml", get(root_desc))
        .route("/ContentDirectory/scpd.xml", get(content_directory_scpd))
        .route("/ConnectionManager/scpd.xml", get(connection_manager_scpd))
        .route("/ContentDirectory/control", post(content_directory_control))
        .route("/ConnectionManager/control", post(connection_manager_control))
        .route("/ContentDirectory/event", any(content_directory_event))
        .route("/ConnectionManager/event", any(connection_manager_event))
        .route("/res/{id}", get(media_by_id))
        .route("/res/{id}/{filename}", get(media_by_name))
        .route("/sub/{id}/{filename}", get(subtitle_by_name))
        // Transcode/remux: isti objekt, ali kroz ffmpeg (profil uredjaja je odlucio).
        .route("/tr/{id}", get(transcode_by_id))
        .route("/tr/{id}/{filename}", get(transcode_by_name))
        .route("/api/status", get(api_status))
        .route("/api/rescan", post(api_rescan))
        .route("/api/devices", get(api_devices))
        .route("/api/profiles", get(api_profiles))
        .route("/api/profiles/reload", post(api_profiles_reload))
        .route("/api/profile/{key}", get(api_profile_for_device).post(crate::api::profiles::save))
        .route("/api/streams", get(api_streams))
        .route("/api/search", get(api_search))
        .route("/api/library", get(api_library))
        .route("/api/continue", get(api_continue))
        .route("/api/playstate/{id}", get(api_playstate).put(api_set_playstate).delete(api_clear_playstate))
        .route("/api/decision/{id}", get(api_decision))
        .route("/art/{id}", get(api_art))
        .route("/api/posters", get(api_posters))
        .route("/api/posters/refresh", post(api_posters_refresh))
        .merge(crate::api::browse::routes())
        .merge(crate::api::fs::routes())
        .merge(crate::api::stats::routes())
        .merge(crate::api::logs::routes())
        .merge(crate::api::settings::routes())
        .merge(crate::api::transcode::routes())
        .merge(crate::assets::routes())
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

// -------------------------------------------------------------- GENA / eventing

async fn content_directory_event(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    eventing(state, gena::CONTENT_DIRECTORY, method, headers).await
}

async fn connection_manager_event(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    eventing(state, gena::CONNECTION_MANAGER, method, headers).await
}

/// `SUBSCRIBE` (nova pretplata ili obnova), `UNSUBSCRIBE`, sve ostalo 405.
async fn eventing(state: AppState, service: &str, method: Method, headers: HeaderMap) -> Response {
    match method.as_str() {
        "SUBSCRIBE" => subscribe(state, service, headers).await,
        "UNSUBSCRIBE" => {
            let Some(sid) = header_str(&headers, "sid") else {
                return precondition_failed("UNSUBSCRIBE bez SID-a");
            };
            if state.gena.unsubscribe(&sid) {
                debug!(service = %service, sid = %sid, "pretplata ukinuta");
                empty_ok()
            } else {
                precondition_failed("nepoznat SID")
            }
        }
        other => {
            warn!(service = %service, method = %other, "nepodrzana metoda na eventing ruti");
            method_not_allowed()
        }
    }
}

async fn subscribe(state: AppState, service: &str, headers: HeaderMap) -> Response {
    let sid = header_str(&headers, "sid");
    let callback = header_str(&headers, "callback");
    let notification_type = header_str(&headers, "nt");
    let timeout = header_str(&headers, "timeout");

    // Obnova postojece pretplate (TV salje samo SID).
    let Some(callback) = callback else {
        let Some(sid) = sid else {
            return precondition_failed("SUBSCRIBE bez CALLBACK i bez SID-a");
        };
        return match state.gena.renew(&sid) {
            Some(_) => {
                debug!(service = %service, sid = %sid, "pretplata obnovljena");
                event_ok(&sid, &gena::timeout_header(timeout.as_deref()))
            }
            None => precondition_failed("nepoznat SID"),
        };
    };

    if sid.is_some() {
        return precondition_failed("SUBSCRIBE ne smije imati i SID i CALLBACK");
    }
    if !notification_type.map(|value| value.eq_ignore_ascii_case("upnp:event")).unwrap_or(false) {
        return precondition_failed("fali NT: upnp:event");
    }
    let callbacks = gena::parse_callbacks(&callback);
    if callbacks.is_empty() {
        return precondition_failed("CALLBACK nije valjan URL");
    }

    let subscription = state.gena.subscribe(service, callbacks);
    debug!(service = %service, sid = %subscription.sid, "nova pretplata");

    // Inicijalni NOTIFY (SEQ 0) — TV tako odmah dobije pocetno stanje varijabli.
    let body = event_body(&state, service);
    let registry = state.gena.clone();
    let pending = subscription.clone();
    tokio::spawn(async move {
        if gena::send_notify(&pending, &body, Duration::from_secs(5)).await {
            registry.bump_seq(&pending.sid);
        } else {
            warn!(sid = %pending.sid, "inicijalni NOTIFY nije prosao — pretplata ostaje na cekanju");
        }
    });

    event_ok(&subscription.sid, &gena::timeout_header(timeout.as_deref()))
}

fn event_body(state: &AppState, service: &str) -> String {
    match service {
        gena::CONNECTION_MANAGER => gena::connection_manager_body(
            &protocol::source_protocol_info(&state.config.library.video_extensions),
            "",
        ),
        _ => gena::content_directory_body(state.update_id()),
    }
}

// ------------------------------------------------------------ SOAP / control

async fn content_directory_control(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
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
            let profile = profile_for(&state, &headers, Some(peer)).await;
            // Metapodaci prije ispisa: bez njih ne znamo treba li uredjaju transcode.
            prefetch_media(&state, &browse_request).await;

            let catalog = state.catalog.read().await;
            let engine = PlaybackEngine::new(
                &profile,
                state.sessions.hw(),
                &state.media_probe,
                state.config.transcode.enabled,
            );
            let art = crate::art::StoreArt::new(state.store.clone(), state.base_url.clone())
                .with_catalog(state.catalog.clone());
            let options = BrowseOptions {
                base_url: &state.base_url,
                max_results: MAX_RESULTS,
                views: state.config.library.views,
                view_list: &state.config.library.view_list,
                language: &state.config.ui.language,
                recent_limit: state.config.library.recent_limit,
                playback: Some(&engine),
                art: Some(&art),
            };
            match browse(&catalog, &browse_request, &options) {
                Ok(outcome) => {
                    debug!(
                        object_id = %browse_request.object_id,
                        total = outcome.total,
                        returned = outcome.returned,
                        profile = %profile.id,
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
        // Prava pretraga nad FTS indeksom: TV-i šalju `dc:title contains "..."` kriterij.
        "Search" => {
            let search_request = rustiio_cds::SearchRequest {
                container_id: arg(&args, "ContainerID").unwrap_or_else(|| "0".to_string()),
                criteria: arg(&args, "SearchCriteria").unwrap_or_default(),
                filter: arg(&args, "Filter").unwrap_or_else(|| "*".to_string()),
                starting_index: arg(&args, "StartingIndex").and_then(|v| v.parse().ok()).unwrap_or(0),
                requested_count: arg(&args, "RequestedCount").and_then(|v| v.parse().ok()).unwrap_or(0),
                sort_criteria: arg(&args, "SortCriteria").unwrap_or_default(),
            };
            let profile = profile_for(&state, &headers, Some(peer)).await;
            let catalog = state.catalog.read().await;
            let engine = PlaybackEngine::new(
                &profile,
                state.sessions.hw(),
                &state.media_probe,
                state.config.transcode.enabled,
            );
            let art = crate::art::StoreArt::new(state.store.clone(), state.base_url.clone())
                .with_catalog(state.catalog.clone());
            let options = BrowseOptions {
                base_url: &state.base_url,
                max_results: MAX_RESULTS,
                views: state.config.library.views,
                view_list: &state.config.library.view_list,
                language: &state.config.ui.language,
                recent_limit: state.config.library.recent_limit,
                playback: Some(&engine),
                art: Some(&art),
            };
            let criteria = rustiio_cds::parse_criteria(&search_request.criteria);
            // FTS upit nad lokalnim indeksom je sub-milisekundni; držimo ga u istoj dretvi
            // kao i Browse (katalog je pod read-lockom pa ga ne možemo poslati u spawn_blocking).
            let search_result =
                rustiio_cds::search_catalog(&state.store, &catalog, &search_request, &options);
            match search_result {
                Ok(outcome) => {
                    info!(
                        criteria = %search_request.criteria,
                        total = outcome.total,
                        returned = outcome.returned,
                        device = %header_str(&headers, "user-agent").unwrap_or_else(|| "-".to_string()),
                        "Search"
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
                Err(err) => {
                    warn!(criteria = %criteria.phrases.join(" "), error = %err.description(), "Search nije uspio");
                    soap_fault_response(err.code(), &err.description())
                }
            }
        }
        "GetSortCapabilities" => xml_response(soap::response(
            &request.service,
            &request.action,
            &format!("      <SortCaps>{}</SortCaps>", sort_capabilities()),
        )),
        "GetSearchCapabilities" => xml_response(soap::response(
            &request.service,
            &request.action,
            &format!("      <SearchCaps>{}</SearchCaps>", rustiio_cds::SEARCH_CAPABILITIES),
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
            "      <RcSID>0</RcSID>\n      <AVTransportID>0</AVTransportID>\n      <ProtocolInfo></ProtocolInfo>\n      <PeerConnectionManager></PeerConnectionManager>\n      <PeerConnectionID>-1</PeerConnectionID>\n      <Direction>Output</Direction>\n      <Status>OK</Status>",
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
        Some(path) => {
            let duration = probe_duration(&state, &path).await;
            rustiio_http::serve_file(&path, &headers, method == Method::HEAD, duration).await
        }
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
    let Some(path) = path else {
        return (StatusCode::NOT_FOUND, "no such object").into_response();
    };

    // Ovdje se vidi tocno sto koji uredjaj trazi (korisno za nove TV profile).
    debug!(
        object_id = %id,
        range = %header_str(headers, "range").unwrap_or_else(|| "-".to_string()),
        time_seek = %header_str(headers, "timeseekrange.dlna.org").unwrap_or_else(|| "-".to_string()),
        transfer_mode = %header_str(headers, "transfermode.dlna.org").unwrap_or_else(|| "-".to_string()),
        device = %header_str(headers, "user-agent").unwrap_or_else(|| "-".to_string()),
        "media zahtjev"
    );

    let duration = probe_duration(state, &path).await;
    rustiio_http::serve_file(&path, headers, head_only, duration).await
}

/// Trajanje fajla preko ffprobe-a (blokirajuce, zato `spawn_blocking`).
async fn probe_duration(state: &AppState, path: &FsPath) -> Option<u64> {
    if !state.config.transcode.probe_duration {
        return None;
    }
    let probe = state.duration_probe.clone();
    let owned: PathBuf = path.to_path_buf();
    tokio::task::spawn_blocking(move || probe.duration_ms(&owned)).await.ok().flatten()
}

// --------------------------------------------------- transcode (ffmpeg stream)

async fn transcode_by_id(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    serve_transcoded(&state, &id, &headers, Some(peer), method == Method::HEAD).await
}

async fn transcode_by_name(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path((id, _filename)): Path<(String, String)>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    serve_transcoded(&state, &id, &headers, Some(peer), method == Method::HEAD).await
}

/// Posluzi objekt kroz ffmpeg — ili original, ako profil kaze da uredjaj moze sam.
async fn serve_transcoded(
    state: &AppState,
    id: &str,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    head_only: bool,
) -> Response {
    let node = {
        let catalog = state.catalog.read().await;
        match catalog.get(id) {
            Some(node) if !node.is_container() => node.clone(),
            _ => return (StatusCode::NOT_FOUND, "no such object").into_response(),
        }
    };

    let profile = profile_for(state, headers, peer).await;

    // Metapodaci su blokirajuci (ffprobe) — prvi zahtjev za novi fajl to plati jednom.
    if !state.media_probe.is_known(&node.path) {
        let probe = state.media_probe.clone();
        let path = node.path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            probe.probe(&path);
        })
        .await;
    }

    let engine = PlaybackEngine::new(
        &profile,
        state.sessions.hw(),
        &state.media_probe,
        state.config.transcode.enabled,
    );
    match engine.decision(&node) {
        // Uredjaj moze original (ili ne znamo dovoljno) — saljemo fajl kakav jest.
        None => return serve_node(state, id, headers, head_only).await,
        Some(decision) if decision.mode == PlaybackMode::Direct => {
            return serve_node(state, id, headers, head_only).await;
        }
        Some(decision) => {
            if let Some((mode, reasons)) = engine.decision_summary(&node) {
                info!(
                    object_id = %id,
                    profile = %profile.id,
                    mode = %mode,
                    reasons = %reasons,
                    device = %header_str(headers, "user-agent").unwrap_or_else(|| "-".to_string()),
                    "transcode"
                );
            }
            serve_ffmpeg(state, &node, &decision, id, headers, peer, head_only).await
        }
    }
}

/// Pokreni ffmpeg i vrati tijelo koje ga drzi na zivotu dok TV gleda.
async fn serve_ffmpeg(
    state: &AppState,
    node: &rustiio_library::Node,
    decision: &rustiio_transcode::Decision,
    id: &str,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    head_only: bool,
) -> Response {
    state.capture.note_stream(&device_key(headers, peer), id);

    // `TimeSeekRange` na transcode streamu = `-ss` prije ulaza (TV premotava film).
    let start_at = header_str(headers, "timeseekrange.dlna.org")
        .and_then(|value| rustiio_http::parse_time_seek(&value))
        .map(|seek| seek.start_ms)
        .filter(|ms| *ms > 0);

    let extension =
        node.path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let request = StartRequest {
        input: &node.path,
        source_ext: &extension,
        decision,
        ffmpeg_path: state.sessions.ffmpeg_path(),
        start_at_ms: start_at,
        subtitle: node.subtitle.as_deref(),
    };

    if head_only {
        return transcode_response(decision, &content_features(&decision.protocol_info), None, start_at);
    }

    let mut session = match state.sessions.start(&request, id, decision.hw.name()).await {
        Ok(session) => session,
        Err(error) => {
            warn!(object_id = %id, %error, "transcode nije pokrenut");
            return (StatusCode::INTERNAL_SERVER_ERROR, format!("transcode: {error}")).into_response();
        }
    };
    let Some(stdout) = session.take_stdout() else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "ffmpeg nije dao izlaz").into_response();
    };

    // Sesija putuje s tijelom: kad TV zatvori stream, ffmpeg se ubija i mjesto se vraca.
    let stream = SessionStream { inner: ReaderStream::new(stdout), _session: session };
    transcode_response(
        decision,
        &content_features(&decision.protocol_info),
        Some(axum::body::Body::from_stream(stream)),
        start_at,
    )
}

/// DLNA `contentFeatures.dlna.org` je dio `protocolInfo`-a nakon cetvrtog dvotocka.
fn content_features(protocol_info: &str) -> String {
    protocol_info.splitn(4, ':').nth(3).unwrap_or_default().to_string()
}

fn transcode_response(
    decision: &rustiio_transcode::Decision,
    content_features: &str,
    body: Option<axum::body::Body>,
    start_at: Option<u64>,
) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, decision.mime.clone())
        .header("transfermode.dlna.org", "Streaming")
        .header("contentfeatures.dlna.org", content_features)
        .header(header::CACHE_CONTROL, "no-store")
        // Duljina streama se ne zna unaprijed, a TV-i ne vole chunked bez ovoga.
        .header(header::CONNECTION, "close");
    if let Some(start_ms) = start_at {
        builder = builder.header(
            "timeseekrange.dlna.org",
            format!("npt={}-", rustiio_http::time_seek::format_npt(start_ms)),
        );
    }
    builder
        .body(body.unwrap_or_else(axum::body::Body::empty))
        .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "response").into_response())
}

/// HTTP tijelo koje drzi ffmpeg sesiju — drop sesije ubija proces.
struct SessionStream {
    inner: ReaderStream<tokio::process::ChildStdout>,
    _session: rustiio_transcode::Session,
}

impl futures_core::Stream for SessionStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_next(cx)
    }
}

// ------------------------------------------------- uredjaji, profili, streamovi

/// Prepoznaj uredjaj iz zaglavlja; zapisi ga (osnova za nove profile).
async fn profile_for(state: &AppState, headers: &HeaderMap, peer: Option<SocketAddr>) -> Profile {
    let identity = device_key(headers, peer);
    let (profile, reasons) = {
        let profiles = state.profiles.read().await;
        let outcome = profiles.identify(&identity);
        (outcome.profile.clone(), outcome.reasons.clone())
    };
    if state.config.profiles.capture {
        let record = state.capture.record(&identity, &profile.id, &dlna_headers(headers));
        // Uređaj se pamti i u bazi: bez toga je stranica Uređaji nakon svakog
        // dizanja servera prazna, iako su TV-i isti.
        if let Err(error) =
            rustiio_library::store::devices::save(&state.store, &crate::state::uredjaj_u_bazu(&record))
        {
            warn!(%error, "ne mogu zapamtiti uredjaj");
        }
    }
    debug!(
        device = %identity.user_agent.clone().unwrap_or_default(),
        profile = %profile.id,
        reasons = %reasons.join("; "),
        "profil uredjaja"
    );
    profile
}

fn device_key(headers: &HeaderMap, peer: Option<SocketAddr>) -> DeviceKey {
    DeviceKey {
        user_agent: header_str(headers, "user-agent"),
        friendly_name: header_str(headers, "x-friendly-name"),
        ip: peer.map(|addr| addr.ip().to_string()),
        device_type: header_str(headers, "x-av-client-type"),
    }
}

/// DLNA zaglavlja koja pamtimo — bez njih se profil pise na pamet.
fn dlna_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    const WANTED: [&str; 12] = [
        "user-agent",
        "transfermode.dlna.org",
        "getcontentfeatures.dlna.org",
        "contentfeatures.dlna.org",
        "timeseekrange.dlna.org",
        "range",
        "x-av-client-type",
        "x-av-transport",
        "playback-capacity",
        "real-time-info",
        "x-dlna-conversion",
        "sony-av-transport",
    ];
    WANTED
        .iter()
        .filter_map(|name| header_str(headers, name).map(|value| (name.to_string(), value)))
        .collect()
}

/// Ucitaj metapodatke za objekte koje cemo ispisati (paralelno, ograniceno).
async fn prefetch_media(state: &AppState, request: &BrowseRequest) {
    if !state.config.transcode.enabled {
        return;
    }
    let limit = 200;
    let paths: Vec<PathBuf> = {
        let catalog = state.catalog.read().await;
        let nodes = match rustiio_cds::views::find(&request.object_id) {
            Some(view) => view.items(&catalog, state.config.library.recent_limit),
            None => catalog.children(&request.object_id),
        };
        nodes
            .into_iter()
            .filter(|node| {
                matches!(node.kind, rustiio_library::NodeKind::Video | rustiio_library::NodeKind::Audio)
            })
            .map(|node| node.path)
            .filter(|path| !state.media_probe.is_known(path))
            .take(limit)
            .collect()
    };
    if paths.is_empty() {
        return;
    }
    debug!(count = paths.len(), object_id = %request.object_id, "pripremam metapodatke");
    for chunk in paths.chunks(4) {
        let mut set = tokio::task::JoinSet::new();
        for path in chunk {
            let probe = state.media_probe.clone();
            let path = path.clone();
            set.spawn_blocking(move || {
                probe.probe(&path);
            });
        }
        while set.join_next().await.is_some() {}
    }
}

/// Uredjaji koji su stvarno nesto trazili (User-Agent, DLNA zaglavlja, profil).
async fn api_devices(State(state): State<AppState>) -> Response {
    let devices: Vec<_> = state
        .capture
        .all()
        .into_iter()
        .map(|record| {
            json!({
                "key": record.key,
                "ip": record.ip,
                "user_agent": record.user_agent,
                "friendly_name": record.friendly_name,
                "profile": record.profile_id,
                "requests": record.requests,
                "first_seen": record.first_seen,
                "last_seen": record.last_seen,
                "streams": record.streams,
                "headers": record.headers,
            })
        })
        .collect();
    axum::Json(json!({ "count": devices.len(), "devices": devices })).into_response()
}

async fn api_profiles(State(state): State<AppState>) -> Response {
    let profiles = state.profiles.read().await;
    let list: Vec<_> = profiles
        .all()
        .iter()
        .map(|profile| {
            json!({
                "id": profile.id,
                "name": profile.name,
                "description": profile.description,
                "containers": profile.video.containers,
                "video_codecs": profile.video.codecs,
                "max_height": profile.video.max_height,
                "audio_codecs": profile.audio.codecs,
                "max_channels": profile.audio.max_channels,
                "subtitle_mode": format!("{:?}", profile.subtitle_mode()).to_ascii_lowercase(),
                "target": {
                    "container": profile.transcode.container,
                    "video_codec": profile.transcode.video_codec,
                    "audio_codec": profile.transcode.audio_codec,
                    "max_bitrate_kbps": profile.transcode.max_bitrate_kbps,
                },
                "rules": {
                    "user_agent": profile.rules.user_agent,
                    "friendly_name": profile.rules.friendly_name,
                    "ip": profile.rules.ip,
                },
            })
        })
        .collect();
    let dir = state.profiles_dir();
    axum::Json(json!({
        "count": list.len(),
        "dir": dir.display().to_string(),
        "generic": profiles.generic().id,
        "profiles": list,
    }))
    .into_response()
}

async fn api_profiles_reload(State(state): State<AppState>) -> Response {
    let count = state.reload_profiles().await;
    axum::Json(json!({ "ok": true, "profiles": count, "dir": state.profiles_dir().display().to_string() }))
        .into_response()
}

/// Gotov profil (TOML) za uredjaj koji je nesto trazio — kopiraj u mapu profila.
async fn api_profile_for_device(State(state): State<AppState>, Path(key): Path<String>) -> Response {
    let Some(record) = state.capture.get(&key) else {
        return (StatusCode::NOT_FOUND, "nepoznat uredjaj").into_response();
    };
    let base = {
        let profiles = state.profiles.read().await;
        profiles.get(&record.profile_id).cloned().unwrap_or_else(|| profiles.generic().clone())
    };
    let id = profile_id_from_key(&key);
    match state.capture.profile_toml(&key, &base, &id) {
        Some(text) => ([("content-type", "text/plain; charset=\"utf-8\"")], text).into_response(),
        None => (StatusCode::INTERNAL_SERVER_ERROR, "profil nije generiran").into_response(),
    }
}

/// Jedan objekt iz baze kao JSON (isto za pretragu, "nastavi gledati" i REST).
fn item_json(item: &rustiio_library::ItemRow) -> serde_json::Value {
    json!({
        "id": item.id,
        "title": item.title,
        "kind": item.kind,
        "path": item.path.to_string_lossy(),
        "file": item.file_name(),
        "size": item.size,
        "duration_ms": item.duration_ms,
        "series": item.series,
        "season": item.season,
        "episode": item.episode,
    })
}

/// Greška iz REST API-ja (uvijek JSON, nikad prazno tijelo).
fn api_error(status: StatusCode, message: &str) -> Response {
    (status, axum::Json(json!({ "ok": false, "error": message }))).into_response()
}

/// Pretraga biblioteke (FTS5): `?q=film&kind=video&limit=50`.
async fn api_search(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let query = params.get("q").cloned().unwrap_or_default();
    let kind = params.get("kind").cloned().filter(|kind| !kind.is_empty());
    let limit = params.get("limit").and_then(|limit| limit.parse::<usize>().ok()).unwrap_or(50).clamp(1, 500);

    let store = state.store.clone();
    let query_for_search = query.clone();
    let found = tokio::task::spawn_blocking(move || match kind.as_deref() {
        Some(kind) => rustiio_library::store::search::search_kind(&store, &query_for_search, kind, limit),
        None => rustiio_library::store::search::search(&store, &query_for_search, limit),
    })
    .await;

    match found {
        Ok(Ok(hits)) => axum::Json(json!({
            "query": query,
            "count": hits.len(),
            "hits": hits.iter().map(|hit| {
                let mut value = item_json(&hit.item);
                value["rank"] = json!(hit.rank);
                value
            }).collect::<Vec<_>>(),
        }))
        .into_response(),
        Ok(Err(error)) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    }
}

/// Što je u bazi: stabilni indeks (brojevi po vrsti, serije, shema).
async fn api_library(State(state): State<AppState>) -> Response {
    let store = state.store.clone();
    let stats = tokio::task::spawn_blocking(move || {
        let counts = store.counts().map_err(|error| error.to_string())?;
        let items = store.item_count().map_err(|error| error.to_string())?;
        let schema = store.schema_version().map_err(|error| error.to_string())?;
        let series = rustiio_library::store::items::series_list(&store).map_err(|error| error.to_string())?;
        Ok::<_, String>((counts, items, schema, series))
    })
    .await;

    match stats {
        Ok(Ok((counts, items, schema, series))) => axum::Json(json!({
            "items": items,
            "schema": schema,
            "kinds": counts.iter().map(|(kind, count)| (kind.clone(), *count)).collect::<HashMap<_, _>>(),
            "series": series.iter().map(|(name, episodes)| json!({ "name": name, "episodes": episodes })).collect::<Vec<_>>(),
        }))
        .into_response(),
        Ok(Err(error)) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    }
}

/// "Nastavi gledati" za uređaj koji pita (`?device=` ili zaglavlja).
async fn api_continue(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let device = params.get("device").cloned().unwrap_or_else(|| crate::library::device_key(&headers));
    let limit = params.get("limit").and_then(|limit| limit.parse::<usize>().ok()).unwrap_or(20).clamp(1, 100);
    let store = state.store.clone();
    let device_for_query = device.clone();

    let found = tokio::task::spawn_blocking(move || {
        rustiio_library::store::play_state::continue_watching(&store, &device_for_query, limit)
    })
    .await;

    match found {
        Ok(Ok(rows)) => axum::Json(json!({
            "device": device,
            "count": rows.len(),
            "items": rows.iter().map(|(item, position)| {
                let mut value = item_json(item);
                value["position_ms"] = json!(position.position_ms);
                value["progress"] = json!(position.progress());
                value["updated_at"] = json!(position.updated_at);
                value
            }).collect::<Vec<_>>(),
        }))
        .into_response(),
        Ok(Err(error)) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    }
}

/// Pozicija uređaja na objektu.
async fn api_playstate(State(state): State<AppState>, Path(id): Path<i64>, headers: HeaderMap) -> Response {
    let device = crate::library::device_key(&headers);
    let store = state.store.clone();
    let device_for_query = device.clone();
    let found = tokio::task::spawn_blocking(move || {
        rustiio_library::store::play_state::get(&store, &device_for_query, id)
    })
    .await;

    match found {
        Ok(Ok(Some(position))) => axum::Json(json!({
            "device": device,
            "item_id": id,
            "position_ms": position.position_ms,
            "duration_ms": position.duration_ms,
            "played": position.played,
            "progress": position.progress(),
            "updated_at": position.updated_at,
        }))
        .into_response(),
        Ok(Ok(None)) => {
            axum::Json(json!({ "device": device, "item_id": id, "position": null })).into_response()
        }
        Ok(Err(error)) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    }
}

/// Zapiši poziciju: `{"position_ms": 120000, "duration_ms": 7200000}`.
async fn api_set_playstate(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> Response {
    let device = body
        .get("device")
        .and_then(|device| device.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| crate::library::device_key(&headers));
    let position_ms = body.get("position_ms").and_then(serde_json::Value::as_i64).unwrap_or(0);
    let duration_ms = body.get("duration_ms").and_then(serde_json::Value::as_i64);
    let played = body.get("played").and_then(serde_json::Value::as_bool).unwrap_or(false);

    let store = state.store.clone();
    let device_for_write = device.clone();
    let written = tokio::task::spawn_blocking(move || {
        if played {
            return rustiio_library::store::play_state::mark_played(
                &store,
                &device_for_write,
                id,
                rustiio_library::Store::now(),
            );
        }
        rustiio_library::store::play_state::set(
            &store,
            &device_for_write,
            id,
            position_ms,
            duration_ms,
            rustiio_library::Store::now(),
        )
    })
    .await;

    match written {
        Ok(Ok(())) => {
            info!(device = %device, item = id, position_ms, "pozicija zapisana");
            axum::Json(json!({ "ok": true, "device": device, "item_id": id, "position_ms": position_ms }))
                .into_response()
        }
        Ok(Err(error)) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    }
}

/// Zaboravi poziciju ("ne nastavljaj").
async fn api_clear_playstate(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Response {
    let device = crate::library::device_key(&headers);
    let store = state.store.clone();
    let device_for_write = device.clone();
    let cleared = tokio::task::spawn_blocking(move || {
        rustiio_library::store::play_state::clear(&store, &device_for_write, id)
    })
    .await;

    match cleared {
        Ok(Ok(())) => axum::Json(json!({ "ok": true, "device": device, "item_id": id })).into_response(),
        Ok(Err(error)) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    }
}

async fn api_streams(State(state): State<AppState>) -> Response {
    let streams = state.sessions.active();
    let hw = state.sessions.hw();
    axum::Json(json!({
        "active": streams.len(),
        "max_concurrent": state.sessions.max_concurrent(),
        "available_slots": state.sessions.available_slots(),
        "hw": hw.summary(),
        "encoders": hw.available.iter().map(|hw| hw.name()).collect::<Vec<_>>(),
        "streams": streams,
    }))
    .into_response()
}

/// Zasto bi ovaj objekt isao kao transcode — za uredjaj koji pita (bez ffmpeg-a).
async fn api_decision(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let node = {
        let catalog = state.catalog.read().await;
        catalog.get(&id).cloned()
    };
    let Some(node) = node else {
        return (StatusCode::NOT_FOUND, "nepoznat objekt").into_response();
    };
    let profile = profile_for(&state, &headers, Some(peer)).await;

    if !state.media_probe.is_known(&node.path) {
        let probe = state.media_probe.clone();
        let path = node.path.clone();
        let _ = tokio::task::spawn_blocking(move || {
            probe.probe(&path);
        })
        .await;
    }

    let engine = PlaybackEngine::new(
        &profile,
        state.sessions.hw(),
        &state.media_probe,
        state.config.transcode.enabled,
    );
    let info = state.media_probe.get(&node.path);
    let summary = engine.decision_summary(&node);
    let file_name = rustiio_upnp::escape_path_segment(&node.file_name());
    axum::Json(json!({
        "object_id": node.id,
        "title": node.title,
        "profile": profile.id,
        "profile_name": profile.name,
        "mode": summary.as_ref().map(|(mode, _)| mode.clone()),
        "reasons": summary.as_ref().map(|(_, reasons)| reasons.clone()),
        "resource": match summary {
            Some((mode, _)) if !mode.starts_with("direct") => PlaybackEngine::transcode_path(&node),
            _ => format!("/res/{}/{file_name}", node.id),
        },
        "media": info.map(|info| json!({
            "container": info.container,
            "duration_ms": info.duration_ms,
            "bitrate_kbps": info.bitrate_kbps,
            "video": info.video.map(|video| json!({
                "codec": video.codec,
                "width": video.width,
                "height": video.height,
                "bitrate_kbps": video.bitrate_kbps,
            })),
            "audio": info.audio.map(|audio| json!({
                "codec": audio.codec,
                "channels": audio.channels,
            })),
        })),
    }))
    .into_response()
}

/// `ua:SEC_HHP_[TV]UE55MU6172/1.0` → `device-sec-hhp-tv-ue55mu6172-1-0`.
fn profile_id_from_key(key: &str) -> String {
    let cleaned: String =
        key.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let mut id = String::from("device-");
    let mut last_dash = false;
    for c in cleaned.chars() {
        if c == '-' {
            if !last_dash {
                id.push('-');
            }
            last_dash = true;
        } else {
            id.push(c);
            last_dash = false;
        }
    }
    let id = id.trim_end_matches('-').to_string();
    id.chars().take(60).collect()
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
        "transcode": {
            "hw": state.sessions.hw().summary(),
            "encoders": state.sessions.hw().available.iter().map(|hw| hw.name()).collect::<Vec<_>>(),
            "encoder": state.sessions.hw().encoder.clone(),
            "threads": state.sessions.hw().threads,
            "hardware_decode": state.sessions.hw().hardware_decode,
            "mode": state.config.transcode.mode,
            "max_concurrent": state.sessions.max_concurrent(),
            "active": state.sessions.active().len(),
        },
        "profiles": {
            "count": state.profiles.read().await.all().len(),
            "dir": state.profiles_dir().display().to_string(),
            "devices": state.capture.len(),
        },
        "views": state.config.library.views,
        "view_list": state.config.library.view_list,
        "ui_language": state.ui_language(),
        "uptime_secs": state.uptime_secs(),
        "update_id": catalog.update_id,
        "items": catalog.len(),
        "media_probed": state.media_probe.len(),
        "counts": counts,
        "roots": roots,
        "subscriptions": state.gena.len(),
    }))
    .into_response()
}

async fn api_rescan(State(state): State<AppState>) -> Response {
    let count = state.rescan().await;
    let update_id = state.update_id();
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

fn empty_ok() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .body(axum::body::Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Uspjeh `SUBSCRIBE` — UPnP trazi `SID` i `TIMEOUT`, bez tijela.
fn event_ok(sid: &str, timeout: &str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("SID", sid)
        .header("TIMEOUT", timeout)
        .header(header::CONTENT_LENGTH, "0")
        .body(axum::body::Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// UPnP: `412 Precondition Failed` kad pretplata nije valjana.
fn precondition_failed(reason: &str) -> Response {
    debug!(reason = %reason, "SUBSCRIBE/UNSUBSCRIBE odbijen");
    Response::builder()
        .status(StatusCode::PRECONDITION_FAILED)
        .body(axum::body::Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn method_not_allowed() -> Response {
    Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .body(axum::body::Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// SOAP fault ide s HTTP 500, kako UPnP spec traza.
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

/// Poster objekta (`upnp:albumArtURI` u DIDL-u pokazuje ovamo).
async fn api_art(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Ok(item_id) = id.parse::<i64>() else {
        return (StatusCode::BAD_REQUEST, "neispravan id").into_response();
    };
    let store = state.store.clone();
    let dir = state.enricher.art_dir().to_path_buf();
    let found =
        tokio::task::spawn_blocking(move || crate::art::serve(&store, &dir, item_id)).await.ok().flatten();
    match found {
        Some((path, content_type)) => match tokio::fs::read(&path).await {
            Ok(bytes) => (
                [
                    (header::CONTENT_TYPE, content_type.to_string()),
                    (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
                ],
                bytes,
            )
                .into_response(),
            Err(_) => (StatusCode::NOT_FOUND, "slika nedostupna").into_response(),
        },
        None => (StatusCode::NOT_FOUND, "nema postera").into_response(),
    }
}

/// Stanje postera: imamo / cekaju / probano bez uspjeha.
async fn api_posters(State(state): State<AppState>) -> Response {
    let store = state.store.clone();
    let stats =
        tokio::task::spawn_blocking(move || rustiio_library::store::items::poster_stats(&store)).await;
    match stats {
        Ok(Ok(stats)) => axum::Json(json!({
            "have": stats.have,
            "pending": stats.pending,
            "none": stats.none,
            "tmdb_key": state.enricher.has_api_key(),
            "art_dir": state.enricher.art_dir().display().to_string(),
        }))
        .into_response(),
        Ok(Err(error)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("baza: {error}")).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, format!("posao: {error}")).into_response(),
    }
}

/// Ponovni prolaz: zaboravi "nema ga" i dohvati sto fali (radi u pozadini).
async fn api_posters_refresh(State(state): State<AppState>) -> Response {
    let store = state.store.clone();
    let enricher = state.enricher.clone();
    // Prvo se obrišu stari posteri (i keširane slike), pa prolaz dohvaća iznova:
    // naslov se razriješi u ID i poster ide ravno na taj zapis kod izvora.
    let reset = tokio::task::spawn_blocking(move || {
        rustiio_library::metadata::forget_video_posters(&store, &enricher)
    })
    .await;
    match reset {
        Ok(Ok(reset)) => {
            let worker = state.clone();
            tokio::task::spawn_blocking(move || {
                crate::state::refresh_posters(&worker, 25, 200);
            });
            axum::Json(json!({ "reset": reset, "started": true })).into_response()
        }
        Ok(Err(error)) => (StatusCode::INTERNAL_SERVER_ERROR, format!("baza: {error}")).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, format!("posao: {error}")).into_response(),
    }
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
            rustiio_profiles::builtin::load(),
        )
    }

    #[test]
    fn router_builds_without_panic() {
        let _ = router(state());
    }

    #[test]
    fn mime_helper_is_reachable() {
        assert_eq!(rustiio_upnp::protocol::mime_for_ext("mkv"), "video/x-matroska");
    }

    #[test]
    fn event_body_follows_the_service() {
        let state = state();
        let cd = event_body(&state, gena::CONTENT_DIRECTORY);
        assert!(cd.contains("SystemUpdateID"), "{cd}");
        let cm = event_body(&state, gena::CONNECTION_MANAGER);
        assert!(cm.contains("SourceProtocolInfo"), "{cm}");
        assert!(!cm.contains("SystemUpdateID"), "{cm}");
    }

    #[tokio::test]
    async fn subscribe_requires_callback_and_nt() {
        let state = state();

        // Bez icega -> 412
        let response = eventing(
            state.clone(),
            gena::CONTENT_DIRECTORY,
            Method::from_bytes(b"SUBSCRIBE").unwrap(),
            HeaderMap::new(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);

        // Samo CALLBACK bez NT -> 412
        let mut headers = HeaderMap::new();
        headers.insert("callback", "<http://127.0.0.1:9/ev>".parse().unwrap());
        let response = eventing(
            state.clone(),
            gena::CONTENT_DIRECTORY,
            Method::from_bytes(b"SUBSCRIBE").unwrap(),
            headers,
        )
        .await;
        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);

        // Valjano -> 200 + SID + TIMEOUT
        let mut headers = HeaderMap::new();
        headers.insert("callback", "<http://127.0.0.1:9/ev>".parse().unwrap());
        headers.insert("nt", "upnp:event".parse().unwrap());
        headers.insert("timeout", "Second-300".parse().unwrap());
        let response = eventing(
            state.clone(),
            gena::CONTENT_DIRECTORY,
            Method::from_bytes(b"SUBSCRIBE").unwrap(),
            headers,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let sid = response.headers().get("sid").unwrap().to_str().unwrap().to_string();
        assert!(sid.starts_with("uuid:rustiio-"));
        assert_eq!(response.headers().get("timeout").unwrap(), "Second-300");
        assert_eq!(state.gena.len(), 1);
    }

    #[tokio::test]
    async fn subscribe_with_sid_renews_and_unknown_sid_is_412() {
        let state = state();
        let subscription =
            state.gena.subscribe(gena::CONTENT_DIRECTORY, vec!["http://127.0.0.1:9/ev".into()]);

        let mut headers = HeaderMap::new();
        headers.insert("sid", subscription.sid.parse().unwrap());
        let response = eventing(
            state.clone(),
            gena::CONTENT_DIRECTORY,
            Method::from_bytes(b"SUBSCRIBE").unwrap(),
            headers,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("sid").unwrap(), subscription.sid.as_str());

        let mut headers = HeaderMap::new();
        headers.insert("sid", "uuid:nepostojeci".parse().unwrap());
        let response = eventing(
            state.clone(),
            gena::CONTENT_DIRECTORY,
            Method::from_bytes(b"SUBSCRIBE").unwrap(),
            headers,
        )
        .await;
        assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn unsubscribe_removes_the_subscription() {
        let state = state();
        let subscription =
            state.gena.subscribe(gena::CONTENT_DIRECTORY, vec!["http://127.0.0.1:9/ev".into()]);

        let mut headers = HeaderMap::new();
        headers.insert("sid", subscription.sid.parse().unwrap());
        let response = eventing(
            state.clone(),
            gena::CONTENT_DIRECTORY,
            Method::from_bytes(b"UNSUBSCRIBE").unwrap(),
            headers,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(state.gena.is_empty());
    }

    #[tokio::test]
    async fn other_methods_are_405() {
        let state = state();
        let response = eventing(state, gena::CONTENT_DIRECTORY, Method::POST, HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
