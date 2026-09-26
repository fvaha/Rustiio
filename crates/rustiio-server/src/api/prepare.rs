//! Priprema videa za uredjaje koji ne podnose DD+/eac3 zvuk — s **redom** za konverziju.
//!
//! - [`api_prepare_ac3`] — stavi posao u **red** (radi se jedan po jedan, u pozadini)
//! - [`api_status`] — cijeli red: sto ceka, sto radi, sto je gotovo, sto je palo
//!
//! ffmpeg **kopira sliku** (bez re-enkodiranja) i pretvara samo zvuk u AC-3 2ch.
//! Kad posao uspije: titlovi (.hr/.en) kopiraju se uz novi fajl, a **original ide u
//! `.off`** — ostaje na disku, ali ga knjiznica ne prikazuje, pa nema duplog videa.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

/// Jedan posao u redu.
#[derive(Clone, serde::Serialize)]
pub struct Posao {
    pub id: String,
    pub title: String,
    pub out: String,
    /// `ceka` | `radi` | `gotovo` | `greska`
    pub state: String,
    pub message: String,
    /// Redni broj prijave — za stabilan poredak u prikazu.
    pub redni: u64,
}

fn red() -> &'static Mutex<VecDeque<Posao>> {
    static R: OnceLock<Mutex<VecDeque<Posao>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn sljedeci_redni() -> u64 {
    static N: OnceLock<AtomicU64> = OnceLock::new();
    N.get_or_init(|| AtomicU64::new(1)).fetch_add(1, Ordering::Relaxed)
}

/// Vrti li radnik (jedan po procesu).
static RADNIK_ZIV: AtomicBool = AtomicBool::new(false);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/prepare/ac3", post(api_prepare_ac3))
        .route("/api/prepare/status", get(api_status))
}

#[derive(Debug, Deserialize)]
struct PrepareBody {
    /// ID objekta iz knjiznice (isti ID koji vidi TV).
    id: String,
}

/// `GET /api/prepare/status` — cijeli red + sazetak za UI.
async fn api_status() -> Response {
    let poslovi: Vec<Posao> = red().lock().map(|r| r.iter().cloned().collect()).unwrap_or_default();
    let ceka = poslovi.iter().filter(|p| p.state == "ceka").count();
    let radi = poslovi.iter().any(|p| p.state == "radi");
    Json(json!({ "ok": true, "poslovi": poslovi, "ceka": ceka, "radi": radi })).into_response()
}

/// `POST /api/prepare/ac3` — stavi u red za konverziju (jedan po jedan).
async fn api_prepare_ac3(State(state): State<AppState>, Json(body): Json<PrepareBody>) -> Response {
    // 1) putanja i naslov iz kataloga (nikad ne primamo putanju izvana)
    let (putanja, naslov) = {
        let catalog = state.catalog.read().await;
        match catalog.get(&body.id) {
            Some(node) if !node.is_container() => (node.path.clone(), node.title.clone()),
            _ => {
                return Json(json!({ "ok": false, "greska": "Nema takvog objekta u knjiznici." }))
                    .into_response();
            }
        }
    };
    if !putanja.is_file() {
        return Json(json!({ "ok": false, "greska": "Izvor nije datoteka." })).into_response();
    }

    // 2) izlaz: isto ime + `.AC3` prije nastavka, u istom direktoriju
    let izlaz = putanja.with_file_name(izlazno_ime(&putanja));
    if izlaz.exists() {
        return Json(json!({
            "ok": false,
            "greska": format!("Kopija vec postoji: {}", izlaz.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())
        }))
        .into_response();
    }

    // 3) u red — bez duplikata (isti naslov koji vec ceka ili se radi)
    let pozicija = {
        let mut r = match red().lock() {
            Ok(r) => r,
            Err(_) => return Json(json!({ "ok": false, "greska": "Red je zakljucan." })).into_response(),
        };
        if r.iter().any(|p| p.id == body.id && (p.state == "ceka" || p.state == "radi")) {
            return Json(json!({ "ok": false, "greska": "Taj naslov je vec u redu za konverziju." }))
                .into_response();
        }
        r.push_back(Posao {
            id: body.id.clone(),
            title: naslov.clone(),
            out: izlaz.display().to_string(),
            state: "ceka".into(),
            message: String::new(),
            redni: sljedeci_redni(),
        });
        r.iter().filter(|p| p.state == "ceka").count()
    };

    pokreni_radnika(state);

    Json(json!({
        "ok": true,
        "pozicija": pozicija,
        "title": naslov,
        "out": izlaz.display().to_string(),
        "poruka": if pozicija > 1 {
            format!("U redu za konverziju — {pozicija}. na redu.")
        } else {
            "Konverzija pocinje odmah.".to_string()
        }
    }))
    .into_response()
}

/// Jedan radnik za cijeli proces: uzima poslove iz reda i radi ih **jedan po jedan**.
fn pokreni_radnika(state: AppState) {
    if RADNIK_ZIV.swap(true, Ordering::SeqCst) {
        return; // vec vrti
    }
    tokio::spawn(async move {
        loop {
            // 1) uzmi sljedeci koji ceka i oznaci ga kao aktivnog
            // (bez .await dok je lock drzan — guard ne smije prijeci preko await-a)
            let posao = match red().lock() {
                Ok(mut r) => match r.iter_mut().find(|p| p.state == "ceka") {
                    Some(p) => {
                        p.state = "radi".into();
                        p.message = "kopiram sliku, pretvaram zvuk u AC-3…".into();
                        Some((p.id.clone(), p.out.clone()))
                    }
                    None => None,
                },
                Err(_) => None,
            };

            let Some((id, izlaz)) = posao else {
                tokio::time::sleep(Duration::from_millis(700)).await;
                continue;
            };

            // 2) odradi ga do kraja (jedan po jedan)
            let (novo_stanje, poruka) = obradi(&state, &id, &izlaz).await;

            if let Ok(mut r) = red().lock() {
                if let Some(p) = r.iter_mut().find(|p| p.id == id) {
                    p.state = novo_stanje;
                    p.message = poruka;
                }
            }

            // 3) knjiznica odmah vidi novi fajl (i nestanak originala u `.off`)
            state.rescan().await;
        }
    });
}

/// Odradi jedan posao: ffmpeg (slika copy, zvuk ac3 2ch), pa titlovi + original u `.off`.
async fn obradi(state: &AppState, id: &str, izlaz: &str) -> (String, String) {
    let putanja = {
        let catalog = state.catalog.read().await;
        catalog.get(id).map(|node| node.path.clone())
    };
    let Some(izvor) = putanja else {
        return ("greska".into(), "objekt je u meduvremenu nestao iz knjiznice".into());
    };

    let ffmpeg = state.config.transcode.ffmpeg_path.clone();
    if ffmpeg.trim().is_empty() {
        return ("greska".into(), "Nema putanje do ffmpeg-a u configu.".into());
    }

    let ulaz = izvor.to_string_lossy().to_string();
    let izlaz_s = izlaz.to_string();
    let rezultat = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&ffmpeg)
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&ulaz)
            .args(["-map", "0:v:0", "-map", "0:a:0?"])
            .args(["-c:v", "copy", "-c:a", "ac3", "-b:a", "192k", "-ac", "2"])
            .args(["-map_metadata", "0", "-f", "matroska"])
            .arg(&izlaz_s)
            .output()
    })
    .await;

    let (uspjelo, poruka) = match rezultat {
        Ok(Ok(out)) if out.status.success() => (true, "Gotovo.".to_string()),
        Ok(Ok(out)) => {
            let greska = String::from_utf8_lossy(&out.stderr).trim().replace('\n', " ");
            let _ = std::fs::remove_file(izlaz); // ne ostavljaj polovicnu datoteku
            (
                false,
                if greska.is_empty() {
                    format!("ffmpeg nije uspio (kod {:?}).", out.status.code())
                } else {
                    greska
                },
            )
        }
        Ok(Err(error)) => (false, format!("ffmpeg se nije mogao pokrenuti: {error}")),
        Err(error) => (false, format!("posao je prekinut: {error}")),
    };

    if !uspjelo {
        return ("greska".into(), poruka);
    }

    // titlovi uz kopiju (pa i ona ima hr/en kao original)
    let osnova = izvor.with_extension("").to_string_lossy().to_string();
    let izlaz_osnova = Path::new(izlaz).with_extension("").to_string_lossy().to_string();
    for jezik in ["hr", "en"] {
        let stari = format!("{osnova}.{jezik}.srt");
        if Path::new(&stari).is_file() {
            let _ = std::fs::copy(&stari, format!("{izlaz_osnova}.{jezik}.srt"));
        }
    }
    // original u `.off` — nema duplog videa u knjiznici
    let _ = std::fs::rename(&izvor, format!("{osnova}.off"));

    ("gotovo".into(), "Gotovo — kopija je u knjiznici, original je u .off.".into())
}

/// Ime izlaza za zadani izvor (koristi i test).
pub fn izlazno_ime(izvor: &Path) -> String {
    let ime = izvor.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    format!("{ime}.AC3.mkv")
}
