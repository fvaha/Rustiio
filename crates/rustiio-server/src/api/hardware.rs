//! Stroj na kojem Rustiio radi: jezgre, memorija, grafička i što od enkodera ima.
//!
//! Sučelje iz ovoga crta izbor „grafička ili procesor" i klizač jezgri — nema
//! smisla nuditi NVENC na stroju koji ga nema, ni tražiti jezgre kad enkodira GPU.

use axum::Router;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use sysinfo::System;

use crate::state::AppState;
use rustiio_transcode::hwaccel::{HwAccel, video_encoder};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/hardware", get(api_hardware))
}

/// Što pojedini ubrzivač stvarno zna (popis iz ffmpeg-a, ne pretpostavka).
fn enkoderi(ubrzivac: HwAccel) -> Vec<&'static str> {
    ["h264", "hevc"]
        .iter()
        .filter_map(|kodek| video_encoder(ubrzivac, kodek))
        .collect()
}

/// Grafička preko `nvidia-smi` (nema je? `null` — transcode tada ide na CPU).
fn graficka() -> Value {
    let izlaz = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output();
    let Ok(izlaz) = izlaz else { return Value::Null };
    if !izlaz.status.success() {
        return Value::Null;
    }
    let tekst = String::from_utf8_lossy(&izlaz.stdout);
    let Some(red) = tekst.lines().next() else { return Value::Null };
    let (ime, vram) = red.split_once(',').unwrap_or((red, ""));
    json!({
        "name": ime.trim(),
        "vram_mb": vram.trim().parse::<u64>().ok(),
    })
}

pub async fn api_hardware(State(state): State<AppState>) -> Response {
    let hw = state.sessions.hw();
    let config = state.config.transcode.clone();

    let mut sistem = System::new();
    sistem.refresh_cpu_usage();
    sistem.refresh_memory();
    let niti = sistem.cpus().len();
    let jezgre = System::physical_core_count().unwrap_or(niti);
    let model = sistem
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .unwrap_or_default();

    let ubrzivaci: Vec<Value> = hw
        .available
        .iter()
        .map(|ubrzivac| {
            json!({
                "id": ubrzivac.name(),
                "encoders": enkoderi(*ubrzivac),
            })
        })
        .collect();

    let graficka = graficka();
    // Grafički način ima smisla samo ako stroj stvarno ima ubrzivač.
    let ima_gpu = hw.preferred != HwAccel::None;
    let nacini: Vec<&str> = if ima_gpu {
        vec!["auto", "gpu", "hybrid", "cpu"]
    } else {
        vec!["auto", "cpu"]
    };

    axum::Json(json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "cpu": { "model": model, "cores": jezgre, "threads": niti },
        "memory_mb": sistem.total_memory() / (1024 * 1024),
        "gpu": graficka,
        "accelerators": ubrzivaci,
        "preferred": hw.preferred.name(),
        "notes": hw.notes,
        "hardware_decode": hw.hardware_decode,
        "subtitles": hw.subtitles_filter,
        "modes": nacini,
        "active": {
            "hw_accel": config.hw_accel,
            "mode": config.mode,
            "encoder": config.encoder,
            "threads": config.threads,
            "hardware_decode": config.hardware_decode,
            "max_concurrent": config.max_concurrent,
        },
    }))
    .into_response()
}
