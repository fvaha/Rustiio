//! Profili iz stvarnih uređaja: zapis s TV-a pretvori u TOML profil.
//!
//! `GET /api/profile/{key}` (u `routes.rs`) daje predložak, a `POST /api/profile/{key}`
//! ga spremi u mapu profila i odmah učita — bez restarta.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use crate::api::logs;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
struct SaveBody {
    /// Ime profila (`samsung-ue55`). Prazno ili neispravno → 400.
    id: Option<String>,
}

/// `POST /api/profile/{key}` — spremi profil od uređaja i ponovno ga učitaj.
pub async fn save(State(state): State<AppState>, Path(key): Path<String>, body: String) -> Response {
    let Some(record) = state.capture.get(&key) else {
        return fail(StatusCode::NOT_FOUND, "nepoznat uredjaj (javi se s TV-a pa osvjezi)");
    };
    let body: SaveBody = if body.trim().is_empty() {
        SaveBody { id: None }
    } else {
        match serde_json::from_str(&body) {
            Ok(body) => body,
            Err(error) => return fail(StatusCode::BAD_REQUEST, format!("neispravno tijelo: {error}")),
        }
    };
    let id = match body.id.as_deref() {
        Some(raw) => slug(raw),
        None => slug(record.friendly_name.as_deref().unwrap_or(&record.user_agent)),
    };
    if id.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "ime profila je prazno nakon čišćenja");
    }

    // Osnova: profil koji je uređaj stvarno dobio (ili općeniti).
    let base = {
        let profiles = state.profiles.read().await;
        profiles.get(&record.profile_id).cloned().unwrap_or_else(|| profiles.generic().clone())
    };
    let Some(text) = state.capture.profile_toml(&key, &base, &id) else {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, "ne mogu sastaviti profil iz zapisa");
    };

    let directory = state.profiles_dir();
    if let Err(error) = std::fs::create_dir_all(&directory) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("ne mogu otvoriti mapu profila: {error}"));
    }
    let file = directory.join(format!("{id}.toml"));
    if let Err(error) = std::fs::write(&file, text) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("ne mogu zapisati profil: {error}"));
    }
    let count = state.reload_profiles().await;
    logs::note(
        "INFO",
        "rustiio_server::api::profiles",
        format!("profil '{id}' spremljen iz uredjaja {key} (ukupno {count})"),
    );
    axum::Json(json!({
        "spremljeno": true,
        "id": id,
        "putanja": file.display().to_string(),
        "profiles": count,
    }))
    .into_response()
}

/// `Samsung UE55 Tu8000` → `samsung-ue55-tu8000`.
fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut dash = false;
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').chars().take(48).collect()
}

fn fail(status: StatusCode, message: impl Into<String>) -> Response {
    (status, axum::Json(json!({ "greska": message.into() }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn slug_cleans_device_names() {
        assert_eq!(slug("Samsung UE55 Tu8000"), "samsung-ue55-tu8000");
        assert_eq!(slug("  DLNADOC/1.50  "), "dlnadoc-1-50");
        assert_eq!(slug("!!! ???"), "");
    }

    #[test]
    fn slug_is_bounded() {
        assert!(slug(&"a".repeat(200)).len() <= 48);
    }
}
