//! Posluzivanje medijskih fajlova: byte-range + DLNA headeri + strimano tijelo.

use std::io::SeekFrom;
use std::path::Path;

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use tracing::debug;

use rustiio_upnp::protocol;

use crate::range::parse_range;

/// Posluzi fajl s diska, postujuci `Range` i DLNA headere iz zahtjeva.
pub async fn serve_file(path: &Path, headers: &HeaderMap, head_only: bool) -> Response {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let info = protocol::guess_for_ext(&ext);

    let meta = match tokio::fs::metadata(path).await {
        Ok(meta) => meta,
        Err(err) => {
            debug!(path = %path.display(), error = %err, "fajl nije dostupan");
            return (StatusCode::NOT_FOUND, "not found").into_response();
        }
    };
    if meta.is_dir() {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let total = meta.len();

    let range_header = headers.get(header::RANGE).and_then(|value| value.to_str().ok());
    let parsed = range_header.and_then(|value| parse_range(value, total));

    // Range je poslan, ali ga ne mozemo zadovoljiti -> 416 s ukupnom velicinom.
    if range_header.is_some() && parsed.is_none() && total > 0 {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{total}"))
            .body(Body::empty())
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    let (start, end) = match parsed {
        Some((start, end)) => (start, end),
        None if total == 0 => (0, 0),
        None => (0, total - 1),
    };
    let length = if total == 0 { 0 } else { end - start + 1 };

    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(err) => {
            debug!(path = %path.display(), error = %err, "otvaranje fajla nije uspjelo");
            return (StatusCode::NOT_FOUND, "not found").into_response();
        }
    };
    if start > 0 && file.seek(SeekFrom::Start(start)).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "seek failed").into_response();
    }

    let body =
        if head_only { Body::empty() } else { Body::from_stream(ReaderStream::new(file.take(length))) };

    let status = if parsed.is_some() { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK };

    // TV-i salju transferMode; ako ga ne posalju, Serviio-kompatibilno "Streaming".
    let transfer_mode = headers
        .get("transfermode.dlna.org")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .unwrap_or("Streaming");

    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, info.mime.clone())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, length.to_string())
        .header("transferMode.dlna.org", transfer_mode)
        .header("contentFeatures.dlna.org", info.content_features());

    if parsed.is_some() {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}"));
    }
    if let Ok(modified) = meta.modified() {
        builder = builder.header(header::LAST_MODIFIED, httpdate::fmt_http_date(modified));
    }

    builder.body(body).unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Kratki wrapper za slucaj kad znamo samo putanju i metodu.
pub async fn serve_node_path(path: &Path, headers: &HeaderMap, head_only: bool) -> Response {
    serve_file(path, headers, head_only).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_file_is_404() {
        let headers = HeaderMap::new();
        let response = serve_file(Path::new("/nema/ovog/fajla.mkv"), &headers, false).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
