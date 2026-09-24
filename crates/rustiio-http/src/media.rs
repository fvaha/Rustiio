//! Posluzivanje medijskih fajlova: byte-range, `TimeSeekRange`, DLNA headeri.

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
use crate::time_seek;

/// Kako cemo posluziti fajl (izracunato prije citanja s diska).
struct Plan {
    start: u64,
    end: u64,
    status: StatusCode,
    /// Dodatni headeri (Content-Range ili TimeSeekRange).
    extras: Vec<(&'static str, String)>,
}

/// Posluzi fajl s diska, postujuci `Range`, `TimeSeekRange` i DLNA headere.
///
/// `duration_ms` dolazi iz `DurationProbe` — bez njega `TimeSeekRange` ne moze
/// pretvoriti vrijeme u bajtove, pa se takav zahtjev ignorira (TV tada dobije
/// cijeli film, sto je valjan odgovor i klijent se sam prebaci na `Range`).
pub async fn serve_file(
    path: &Path,
    headers: &HeaderMap,
    head_only: bool,
    duration_ms: Option<u64>,
) -> Response {
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

    let time_seek_request = headers
        .get("timeseekrange.dlna.org")
        .and_then(|value| value.to_str().ok())
        .and_then(time_seek::parse_time_seek);
    let duration = duration_ms.filter(|value| *value > 0);

    let plan = match (time_seek_request, duration) {
        (Some(seek), Some(duration)) if total > 0 => {
            // Trazenje preko kraja filma (TV zna poslati npt dulji od trajanja)
            // svodi se na zadnji bajt — nikad prazno tijelo.
            let last = total - 1;
            let start = time_seek::byte_for_time(seek.start_ms, duration, total).min(last);
            let end_ms = seek.end_ms.unwrap_or(duration).min(duration);
            let end = if seek.end_ms.is_some() {
                time_seek::byte_for_time(end_ms, duration, total).clamp(start, last)
            } else {
                last
            };
            Plan {
                start,
                end,
                // DLNA spec: odgovor na TimeSeekRange nije 206 nego 200 + header.
                status: StatusCode::OK,
                extras: vec![(
                    "TimeSeekRange.dlna.org",
                    time_seek::response_header(seek.start_ms, end_ms, duration),
                )],
            }
        }
        _ => match range_plan(headers, total) {
            Ok(plan) => plan,
            Err(response) => return *response,
        },
    };

    let length = if total == 0 { 0 } else { plan.end - plan.start + 1 };

    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(err) => {
            debug!(path = %path.display(), error = %err, "otvaranje fajla nije uspjelo");
            return (StatusCode::NOT_FOUND, "not found").into_response();
        }
    };
    if plan.start > 0 && file.seek(SeekFrom::Start(plan.start)).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "seek failed").into_response();
    }

    let body =
        if head_only { Body::empty() } else { Body::from_stream(ReaderStream::new(file.take(length))) };

    // TV-i salju transferMode; ako ga ne posalju, Serviio-kompatibilno "Streaming".
    let transfer_mode = headers
        .get("transfermode.dlna.org")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .unwrap_or("Streaming");

    let mut builder = Response::builder()
        .status(plan.status)
        .header(header::CONTENT_TYPE, info.mime.clone())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, length.to_string())
        .header("transferMode.dlna.org", transfer_mode)
        .header("contentFeatures.dlna.org", info.content_features());

    for (name, value) in &plan.extras {
        builder = builder.header(*name, value);
    }
    if let Ok(modified) = meta.modified() {
        builder = builder.header(header::LAST_MODIFIED, httpdate::fmt_http_date(modified));
    }

    builder.body(body).unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Byte-range put: `206` za ispravan range, `416` za nemoguc, `200` za cijeli film.
fn range_plan(headers: &HeaderMap, total: u64) -> Result<Plan, Box<Response>> {
    let range_header = headers.get(header::RANGE).and_then(|value| value.to_str().ok());
    let parsed = range_header.and_then(|value| parse_range(value, total));

    if range_header.is_some() && parsed.is_none() && total > 0 {
        return Err(Box::new(
            Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                .body(Body::empty())
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        ));
    }

    Ok(match parsed {
        Some((start, end)) => Plan {
            start,
            end,
            status: StatusCode::PARTIAL_CONTENT,
            extras: vec![(header::CONTENT_RANGE.as_str(), format!("bytes {start}-{end}/{total}"))],
        },
        None if total == 0 => Plan { start: 0, end: 0, status: StatusCode::OK, extras: Vec::new() },
        None => Plan { start: 0, end: total - 1, status: StatusCode::OK, extras: Vec::new() },
    })
}

/// Kratki wrapper za slucaj kad znamo samo putanju i metodu.
pub async fn serve_node_path(
    path: &Path,
    headers: &HeaderMap,
    head_only: bool,
    duration_ms: Option<u64>,
) -> Response {
    serve_file(path, headers, head_only, duration_ms).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{HeaderName, HeaderValue};

    fn temp_file(tag: &str, bytes: usize) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rustiio-http-{tag}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("film.mkv");
        let content: Vec<u8> = (0..bytes).map(|index| (index % 251) as u8).collect();
        std::fs::write(&path, content).unwrap();
        path
    }

    fn headers_with(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(HeaderName::from_static(name), HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    async fn body_bytes(response: Response) -> Vec<u8> {
        to_bytes(response.into_body(), 1_000_000).await.unwrap().to_vec()
    }

    #[tokio::test]
    async fn missing_file_is_404() {
        let response = serve_file(Path::new("/nema/ovog/fajla.mkv"), &HeaderMap::new(), false, None).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn plain_request_streams_the_whole_file() {
        let path = temp_file("plain", 1000);
        let response = serve_file(&path, &HeaderMap::new(), false, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::CONTENT_LENGTH).unwrap(), "1000");
        assert_eq!(response.headers().get(header::ACCEPT_RANGES).unwrap(), "bytes");
        assert_eq!(body_bytes(response).await.len(), 1000);
    }

    #[tokio::test]
    async fn byte_range_returns_206_with_content_range() {
        let path = temp_file("range", 1000);
        let headers = headers_with(&[("range", "bytes=100-199")]);
        let response = serve_file(&path, &headers, false, None).await;
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers().get(header::CONTENT_RANGE).unwrap(), "bytes 100-199/1000");
        assert_eq!(response.headers().get(header::CONTENT_LENGTH).unwrap(), "100");
        let body = body_bytes(response).await;
        assert_eq!(body.len(), 100);
        assert_eq!(body[0], 100, "prvi bajt mora biti s pomaka 100");
    }

    #[tokio::test]
    async fn unsatisfiable_range_is_416() {
        let path = temp_file("bad-range", 1000);
        let headers = headers_with(&[("range", "bytes=5000-")]);
        let response = serve_file(&path, &headers, false, None).await;
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(response.headers().get(header::CONTENT_RANGE).unwrap(), "bytes */1000");
    }

    #[tokio::test]
    async fn time_seek_returns_slice_and_dlna_header() {
        let path = temp_file("timeseek", 1000);
        // 100 s trajanja, seek na 50 s -> pola fajla, i to od bajta 500.
        let headers = headers_with(&[("timeseekrange.dlna.org", "npt=00:00:50-")]);
        let response = serve_file(&path, &headers, false, Some(100_000)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("timeseekrange.dlna.org").unwrap(),
            "npt=0:00:50.000-0:01:40.000/0:01:40.000"
        );
        assert_eq!(response.headers().get(header::CONTENT_LENGTH).unwrap(), "500");
        let body = body_bytes(response).await;
        assert_eq!(body.len(), 500);
        assert_eq!(body[0], 249, "pomak 500 -> 500 % 251 = 249");
    }

    #[tokio::test]
    async fn time_seek_beyond_duration_serves_the_last_byte() {
        let path = temp_file("timeseek-past-end", 1000);
        let headers = headers_with(&[("timeseekrange.dlna.org", "npt=00:05:00-")]);
        let response = serve_file(&path, &headers, false, Some(5_000)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::CONTENT_LENGTH).unwrap(), "1");
        assert_eq!(body_bytes(response).await.len(), 1);
    }

    #[tokio::test]
    async fn time_seek_without_duration_falls_back_to_full_file() {
        let path = temp_file("no-duration", 1000);
        let headers = headers_with(&[("timeseekrange.dlna.org", "npt=00:00:50-")]);
        let response = serve_file(&path, &headers, false, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("timeseekrange.dlna.org").is_none());
        assert_eq!(response.headers().get(header::CONTENT_LENGTH).unwrap(), "1000");
    }

    #[tokio::test]
    async fn head_request_has_headers_but_no_body() {
        let path = temp_file("head", 1000);
        let response = serve_file(&path, &HeaderMap::new(), true, None).await;
        assert_eq!(response.headers().get(header::CONTENT_LENGTH).unwrap(), "1000");
        assert!(body_bytes(response).await.is_empty());
    }
}
