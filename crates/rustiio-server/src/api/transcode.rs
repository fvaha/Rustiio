//! Sken sustava i automatsko podešavanje transcodea iz sučelja.
//!
//! - [`api_scan`]  — što stroj ima: ffmpeg, CPU, GPU, izmjerena brzina svakog enkodera
//! - [`api_apply`] — upiši preporuku (ili traženi način rada) u config
//!
//! Sken pokreće ffmpeg (testno enkodiranje), pa ide u `spawn_blocking` — inače bi
//! blokirao async runtime i usporio streaming.

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use rustiio_core::config::Config;
use rustiio_transcode::scan_system;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/transcode/scan", get(api_scan)).route("/api/transcode/apply", post(api_apply))
}

/// `GET /api/transcode/scan` — cijeli izvještaj skena + što je sada u pogonu.
async fn api_scan(State(state): State<AppState>) -> Response {
    let (ffmpeg, ffprobe, mode) = {
        let config = Config::load(&state.config_path).unwrap_or_else(|_| state.config.as_ref().clone());
        (config.transcode.ffmpeg_path, config.transcode.ffprobe_path, config.transcode.mode)
    };
    let report = match tokio::task::spawn_blocking(move || scan_system(&ffmpeg, &ffprobe, &mode)).await {
        Ok(report) => report,
        Err(error) => {
            return Json(json!({ "greska": format!("sken nije dovršen: {error}") })).into_response();
        }
    };
    let hw = state.sessions.hw();
    let u_pogonu = json!({
        "ffmpeg": state.config.transcode.ffmpeg_path,
        "mode": state.config.transcode.mode,
        "encoder": hw.encoder.clone(),
        "threads": hw.threads,
        "hardware_decode": hw.hardware_decode,
        "max_concurrent": state.config.transcode.max_concurrent,
        "subtitles_filter": hw.subtitles_filter,
    });
    Json(json!({ "sken": report, "u_pogonu": u_pogonu })).into_response()
}

#[derive(Debug, Deserialize, Default)]
struct ApplyBody {
    /// `auto` | `gpu` | `cpu` | `hybrid`; prazno = auto.
    #[serde(default)]
    mode: Option<String>,
    /// Ako je zadano, upiši točno te vrijednosti umjesto preporuke.
    #[serde(default)]
    encoder: Option<String>,
    #[serde(default)]
    threads: Option<u32>,
    #[serde(default)]
    hardware_decode: Option<bool>,
}

/// `POST /api/transcode/apply` — skeniraj i upiši najbolje u config.
async fn api_apply(State(state): State<AppState>, body: Option<Json<ApplyBody>>) -> Response {
    let body = body.map(|Json(value)| value).unwrap_or_default();
    let mut config = match Config::load(&state.config_path) {
        Ok(config) => config,
        Err(error) => {
            return Json(json!({ "greska": format!("config se ne može pročitati: {error}") }))
                .into_response();
        }
    };

    let mode = body.mode.clone().unwrap_or_else(|| config.transcode.mode.clone());
    let (ffmpeg, ffprobe) = (config.transcode.ffmpeg_path.clone(), config.transcode.ffprobe_path.clone());
    let (ffmpeg, ffprobe, mode_za_sken) = (ffmpeg, ffprobe, mode.clone());
    let report =
        match tokio::task::spawn_blocking(move || scan_system(&ffmpeg, &ffprobe, &mode_za_sken)).await {
            Ok(report) => report,
            Err(error) => {
                return Json(json!({ "greska": format!("sken nije dovršen: {error}") })).into_response();
            }
        };

    // Ručno zadane vrijednosti imaju prednost pred preporukom.
    let preporuka = report.recommendation.clone();
    let encoder =
        body.encoder.filter(|value| !value.trim().is_empty()).unwrap_or_else(|| preporuka.encoder.clone());
    let threads = body.threads.unwrap_or(preporuka.threads);
    let hardware_decode = body.hardware_decode.unwrap_or(preporuka.hardware_decode);

    let mut promjene: Vec<String> = Vec::new();
    let upisi = |ime: &str, staro: String, novo: String, promjene: &mut Vec<String>| {
        if staro != novo {
            promjene.push(format!("{ime}: {staro} → {novo}"));
        }
    };
    upisi("ffmpeg", config.transcode.ffmpeg_path.clone(), preporuka.ffmpeg_path.clone(), &mut promjene);
    upisi("ffprobe", config.transcode.ffprobe_path.clone(), preporuka.ffprobe_path.clone(), &mut promjene);
    upisi("enkoder", config.transcode.encoder.clone(), encoder.clone(), &mut promjene);
    upisi("niti", config.transcode.threads.to_string(), threads.to_string(), &mut promjene);
    upisi(
        "hardversko dekodiranje",
        config.transcode.hardware_decode.to_string(),
        hardware_decode.to_string(),
        &mut promjene,
    );
    upisi("način rada", config.transcode.mode.clone(), preporuka.mode.clone(), &mut promjene);
    upisi(
        "istovremenih",
        config.transcode.max_concurrent.to_string(),
        preporuka.max_concurrent.to_string(),
        &mut promjene,
    );

    config.transcode.ffmpeg_path = preporuka.ffmpeg_path.clone();
    config.transcode.ffprobe_path = preporuka.ffprobe_path.clone();
    config.transcode.encoder = encoder;
    config.transcode.threads = threads;
    config.transcode.hardware_decode = hardware_decode;
    config.transcode.mode = preporuka.mode.clone();
    config.transcode.max_concurrent = preporuka.max_concurrent;

    if let Err(error) = config.save(&state.config_path) {
        return Json(json!({ "greska": format!("upis nije uspio: {error}") })).into_response();
    }
    // Ubrzanje se bira pri dizanju (testno enkodiranje), pa vrijedi nakon restarta.
    tracing::info!(promjene = ?promjene, enkoder = %config.transcode.encoder, "transcode podešen skenom");

    Json(json!({
        "status": "ok",
        "promjene": promjene,
        "restart_potreban": !promjene.is_empty(),
        "preporuka": preporuka,
        "sken": report,
    }))
    .into_response()
}

/// Odabir načina rada bez ponovnog skena (npr. korisnik zna da želi procesor).
pub fn apply_to_config(config: &mut Config, encoder: &str, threads: u32, hardware_decode: bool, mode: &str) {
    config.transcode.encoder = encoder.to_string();
    config.transcode.threads = threads;
    config.transcode.hardware_decode = hardware_decode;
    config.transcode.mode = mode.to_string();
    config.transcode.max_concurrent =
        if mode == "gpu" { 2 } else { config.transcode.max_concurrent.clamp(1, 4) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_writes_the_chosen_values() {
        let mut config = Config::default();
        apply_to_config(&mut config, "h264_nvenc", 0, true, "gpu");
        assert_eq!(config.transcode.encoder, "h264_nvenc");
        assert_eq!(config.transcode.threads, 0);
        assert!(config.transcode.hardware_decode);
        assert_eq!(config.transcode.max_concurrent, 2);
    }

    #[test]
    fn apply_keeps_cpu_mode_threads() {
        let mut config = Config::default();
        apply_to_config(&mut config, "libx264", 6, false, "cpu");
        assert_eq!(config.transcode.threads, 6);
        assert!(!config.transcode.hardware_decode);
        assert_eq!(config.transcode.max_concurrent, 2);
    }
}
