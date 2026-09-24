//! `/api/settings` — config iz browsera, uz pošteno rečeno što traži restart.
//!
//! Config se u radu **ne** mijenja u hodu: ono što se može primijeniti odmah
//! (mape, jezik) traži ponovni pregled mapa, a ono što ne može (port, bind, udn,
//! obitelj IP-a) traži restart servisa — zato `POST /api/restart`.

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use rustiio_core::config::Config;
use serde_json::{Value, json};

use crate::state::AppState;

/// Polja koja se primjenjuju **bez** restarta. Sve ostalo (config se u radu ne
/// mijenja — mape, transcode, port, UDN, obitelj IP-a drže se u `AppState`)
/// vrijedi tek kad se servis digne.
const HOT_PREFIXES: [&str; 1] = ["ui."];

/// Rute: čitanje, pisanje i restart.
pub fn routes() -> Router<AppState> {
    Router::new().route("/api/settings", get(api_get).put(api_put)).route("/api/restart", post(api_restart))
}

/// `GET /api/settings` — config kakav je **na disku** (to sučelje uređuje), plus
/// popis polja koja se razlikuju od pokrenutog procesa (`ceka_restart`).
async fn api_get(State(state): State<AppState>) -> impl IntoResponse {
    // Datoteka je izvor istine: netko je mogao spremiti iz sučelja ili ručno, a
    // proces još radi po starom. Razlika se pošteno prijavi, ne prešućuje.
    let file = Config::load(&state.config_path).unwrap_or_else(|_| state.config.as_ref().clone());
    let pending = diff_paths(state.config.as_ref(), &file);
    let config = serde_json::to_value(&file).unwrap_or(Value::Null);
    axum::Json(json!({
        "config": config,
        "putanja": state.config_path.to_string_lossy(),
        "ceka_restart": pending,
        "bez_restarta": HOT_PREFIXES,
    }))
}

/// `PUT /api/settings` — tijelo je cijeli config (UI ga dobije iz GET-a i vrati izmijenjenog).
///
/// Vraća `promijenjena` (popis polja koja su se razlikovala) i `restart_potreban`.
async fn api_put(State(state): State<AppState>, body: String) -> impl IntoResponse {
    let incoming: Config = match serde_json::from_str(&body) {
        Ok(config) => config,
        Err(error) => return fail(StatusCode::BAD_REQUEST, format!("ne mogu pročitati config: {error}")),
    };
    if let Err(problem) = validate(&incoming) {
        return fail(StatusCode::BAD_REQUEST, problem);
    }

    let changed = diff_paths(state.config.as_ref(), &incoming);
    if let Err(error) = incoming.save(&state.config_path) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("ne mogu spremiti config: {error}"));
    }

    let restart = changed.iter().any(|path| needs_restart(path));
    let traze_restart: Vec<String> = changed.iter().filter(|path| needs_restart(path)).cloned().collect();
    crate::api::logs::note(
        "INFO",
        "rustiio_server::api::settings",
        format!("config spremljen iz web sučelja (promijenjeno: {})", changed.join(", ")),
    );
    axum::Json(json!({
        "spremljeno": true,
        "promijenjena": changed,
        "traze_restart": traze_restart,
        "restart_potreban": restart,
        "putanja": state.config_path.to_string_lossy(),
    }))
    .into_response()
}

/// `POST /api/restart` — izađi iz procesa; systemd (`Restart=always`) ga digne u sekundi.
///
/// Bez autorizacije — sučelje je zasad LAN-only (prijava/tokeni dolaze u Fazi 7).
async fn api_restart() -> impl IntoResponse {
    crate::api::logs::note("WARN", "rustiio_server::api::settings", "restart zatražen iz web sučelja");
    tokio::spawn(async {
        // Kratka stanka da odgovor stigne do browsera prije nego proces padne.
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        std::process::exit(0);
    });
    axum::Json(json!({ "restart": true, "poruka": "servis se diže za nekoliko sekundi" }))
}

/// Osnovna pravila — bolje reći grešku nego spremiti config s kojim se server ne diže.
fn validate(config: &Config) -> Result<(), String> {
    if config.server.http_port == 0 {
        return Err("http_port ne smije biti 0".into());
    }
    if config.server.bind.trim().is_empty() {
        return Err("bind ne smije biti prazan".into());
    }
    if config.server.udn.as_deref().unwrap_or("").trim().is_empty() {
        return Err("udn ne smije biti prazan (TV-i po njemu pamte server)".into());
    }
    for root in &config.library.roots {
        if !root.path.is_absolute() {
            return Err(format!("mapa mora biti apsolutna putanja: {}", root.path.display()));
        }
    }
    Ok(())
}

/// Sva polja koja se razlikuju, kao putanje (`server.http_port`, `library.roots[0]`).
fn diff_paths(old: &Config, new: &Config) -> Vec<String> {
    let (Ok(old), Ok(new)) = (serde_json::to_value(old), serde_json::to_value(new)) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    walk("", &old, &new, &mut paths);
    paths
}

fn walk(prefix: &str, old: &Value, new: &Value, out: &mut Vec<String>) {
    match (old, new) {
        (Value::Object(old), Value::Object(new)) => {
            let keys: std::collections::BTreeSet<&String> = old.keys().chain(new.keys()).collect();
            for key in keys {
                let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                walk(&path, old.get(key).unwrap_or(&Value::Null), new.get(key).unwrap_or(&Value::Null), out);
            }
        }
        (Value::Array(old), Value::Array(new)) => {
            if old.len() != new.len() {
                out.push(prefix.to_string());
                return;
            }
            for (index, (old, new)) in old.iter().zip(new).enumerate() {
                walk(&format!("{prefix}[{index}]"), old, new, out);
            }
        }
        (old, new) if old != new => out.push(prefix.to_string()),
        _ => {}
    }
}

fn needs_restart(path: &str) -> bool {
    !HOT_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

fn fail(status: StatusCode, message: impl Into<String>) -> axum::response::Response {
    (status, axum::Json(json!({ "greska": message.into() }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustiio_core::config::{Root, RootKind};

    fn config() -> Config {
        let mut config = Config::default();
        config.server.http_port = 8200;
        config.server.udn = Some("test-udn".into());
        config.library.roots = vec![root("/media")];
        config
    }

    fn root(path: &str) -> Root {
        Root { label: "test".into(), path: path.into(), kind: RootKind::Video }
    }

    #[test]
    fn valid_config_passes() {
        assert!(validate(&config()).is_ok());
    }

    #[test]
    fn port_zero_is_rejected() {
        let mut broken = config();
        broken.server.http_port = 0;
        assert!(validate(&broken).is_err());
    }

    #[test]
    fn relative_root_is_rejected() {
        let mut broken = config();
        broken.library.roots = vec![root("media/filmovi")];
        assert!(validate(&broken).unwrap_err().contains("apsolutna"));
    }

    #[test]
    fn empty_udn_is_rejected() {
        let mut broken = config();
        broken.server.udn = Some("  ".into());
        assert!(validate(&broken).is_err());
    }

    #[test]
    fn identical_config_has_no_diff() {
        assert!(diff_paths(&config(), &config()).is_empty());
    }

    #[test]
    fn port_change_is_detected_and_needs_restart() {
        let mut new = config();
        new.server.http_port = 9000;
        let changed = diff_paths(&config(), &new);
        assert_eq!(changed, vec!["server.http_port"]);
        assert!(needs_restart(&changed[0]));
    }

    #[test]
    fn language_change_is_detected_and_needs_no_restart() {
        let mut new = config();
        new.ui.language = "en".into();
        let changed = diff_paths(&config(), &new);
        assert_eq!(changed, vec!["ui.language"]);
        assert!(!needs_restart(&changed[0]));
    }

    #[test]
    fn library_change_needs_restart_because_config_is_frozen_at_startup() {
        let mut new = config();
        new.library.posters = !new.library.posters;
        let changed = diff_paths(&config(), &new);
        assert_eq!(changed, vec!["library.posters"]);
        assert!(needs_restart(&changed[0]));
    }

    #[test]
    fn disk_config_is_the_source_of_truth_for_get() {
        // GET mora pokazati ono što je na disku, a `ceka_restart` razliku prema procesu.
        let dir = std::env::temp_dir().join(format!("rustiio-settings-get-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let mut file = config();
        file.server.log_level = "debug".into();
        file.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.server.log_level, "debug");
        assert_eq!(diff_paths(&config(), &loaded), vec!["server.log_level"]);
        assert!(needs_restart("server.log_level"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn added_root_is_reported_by_index() {
        let mut new = config();
        new.library.roots = vec![root("/media"), root("/filmovi")];
        let changed = diff_paths(&config(), &new);
        assert_eq!(changed, vec!["library.roots"]);
    }
}
