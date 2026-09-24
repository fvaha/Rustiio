//! Web sučelje iz binarnog fajla: `web/dist` je ugrađen preko `rust-embed`.
//!
//! Nema vanjskih datoteka — `rustiio` je jedan fajl, a sučelje se servira s `/`.
//! Nepoznata putanja bez nastavka je SPA ruta (`/library`, `/devices`) i dobiva
//! `index.html`; nepoznata datoteka dobiva 404 da browser ne parsira HTML kao sliku.

use axum::Router;
use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use rust_embed::RustEmbed;

use crate::state::AppState;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Web;

/// `/` i sve ostalo (SPA).
pub fn routes() -> Router<AppState> {
    Router::new().route("/", get(index)).route("/{*path}", get(serve))
}

async fn index() -> Response {
    file("index.html")
}

async fn serve(Path(path): Path<String>) -> Response {
    file(&path)
}

/// Posluži datoteku iz ugrađenog `dist`.
fn file(path: &str) -> Response {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    // Nepoznata API ruta mora ostati JSON greška, ne HTML sučelja.
    if path.starts_with("api/") {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({ "greska": format!("nema rute /{path}") })),
        )
            .into_response();
    }
    if let Some(asset) = Web::get(path) {
        return build(path, asset.data.into_owned());
    }
    if std::path::Path::new(path).extension().is_some() {
        return (StatusCode::NOT_FOUND, "nema datoteke").into_response();
    }
    match Web::get("index.html") {
        Some(asset) => build("index.html", asset.data.into_owned()),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "web sučelje nije ugrađeno — u mapi `web` pokreni: npm install && npm run build",
        )
            .into_response(),
    }
}

fn build(path: &str, body: Vec<u8>) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache = if path.starts_with("assets/") {
        // Vite imena sadrže hash sadržaja → smije dugo u keš.
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    ([(header::CONTENT_TYPE, mime.as_ref()), (header::CACHE_CONTROL, cache)], body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_is_served_from_embedded_dist() {
        // `web/dist/index.html` mora postojati u repou (placeholder prije builda).
        let response = file("index.html");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn spa_route_falls_back_to_index() {
        let response = file("library");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn missing_file_is_404_not_html() {
        let response = file("nema-ovo.svg");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn unknown_api_route_is_json_404() {
        let response = file("api/nema-ovo");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
