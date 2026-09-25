//! Spajanje skena s bazom: stabilni DLNA id-evi i upis metapodataka.
//!
//! Zašto: DLNA `ObjectID` koji TV zapamti mora značiti isti film i nakon restarta
//! i nakon reskena. Zato id ne dodjeljuje skener (redni broj), nego baza (po putanji).

use rustiio_library::{Catalog, Store};
use tracing::{info, warn};

pub use rustiio_library::SyncSummary;

/// Upiši katalog u bazu i prebaci id-eve kataloga na id-eve iz baze.
///
/// Tanka ljuska nad `rustiio_library::adopt_catalog` — logika prihvata skena je
/// u biblioteci (čista operacija nad `Catalog` + `Store`), server je samo zove.
pub fn sync_catalog(store: &Store, catalog: &mut Catalog) -> SyncSummary {
    rustiio_library::adopt_catalog(store, catalog)
}

/// Prefila `MediaProbe` cache iz baze — nakon restarta nema ponovnog ffprobe-a.
pub fn warm_probe_cache(store: &Store, probe: &rustiio_library::MediaProbe) -> usize {
    let rows = match rustiio_library::store::items::media_for_probe(store) {
        Ok(rows) => rows,
        Err(error) => {
            warn!(error = %error, "metapodaci iz baze nisu procitani");
            return 0;
        }
    };
    let count = rows.len();
    for (path, info) in rows {
        probe.remember(&path, Some(info));
    }
    if count > 0 {
        info!(count, "metapodaci iz baze u cacheu");
    }
    count
}

/// Dopuni dubinu boje u bazi za zapise upisane prije sheme 5.
///
/// Skener pamti kodek, ali ne i dubinu; bez nje 10-bit HEVC (`Main 10`) izgleda
/// kao običan HEVC, odluka kaže „TV to može", a TV onda ne otvori fajl.
pub fn backfill_bit_depth(store: &Store, probe: &rustiio_library::MediaProbe) -> usize {
    let rows = match rustiio_library::store::items::media_without_bit_depth(store) {
        Ok(rows) => rows,
        Err(error) => {
            warn!(error = %error, "zapisi bez dubine boje nisu procitani");
            return 0;
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|trajanje| trajanje.as_secs() as i64)
        .unwrap_or(0);
    let mut updated = 0;
    for (id, path) in rows {
        // Keš je punjen iz baze — tamo dubine boje još nema, pa bi `probe` vratio
        // upravo taj prazan zapis i ffprobe se ne bi pokrenuo. Zato prvo zaboravi.
        probe.forget(&path);
        let Some(info) = probe.probe(&path) else {
            continue;
        };
        if info.video.as_ref().and_then(|video| video.pix_fmt.as_ref()).is_none() {
            continue;
        }
        if rustiio_library::store::items::update_media(store, id, &info, now).is_ok() {
            updated += 1;
        }
    }
    updated
}

/// Ključ uređaja za watch-state: UDN iz DLNA zaglavlja ako ga ima, inače `User-Agent`.
pub fn device_key(headers: &axum::http::HeaderMap) -> String {
    for name in ["x-av-client-udn", "x-udn", "x-av-client-id"] {
        if let Some(value) = headers.get(name).and_then(|value| value.to_str().ok()) {
            if !value.trim().is_empty() {
                return value.trim().to_string();
            }
        }
    }
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|agent| agent.trim().to_string())
        .unwrap_or_else(|| "nepoznat".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustiio_core::config::{Root, RootKind};
    use rustiio_library::NodeKind;
    use rustiio_library::ScanOptions;
    use rustiio_library::scan::scan;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = format!(
            "rustiio-library-{}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            name
        );
        let path = std::env::temp_dir().join(unique);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("Serije/S01")).expect("mape");
        std::fs::write(path.join("Test Film (2026).mkv"), b"film").expect("film");
        std::fs::write(path.join("Serije/S01/Zlo S01E03.mkv"), b"epizoda").expect("epizoda");
        path
    }

    fn catalog_for(path: &std::path::Path) -> Catalog {
        let options = ScanOptions::new(
            vec![Root { label: "Filmovi".to_string(), path: path.to_path_buf(), kind: RootKind::Video }],
            vec!["mkv".to_string()],
        );
        scan(&options)
    }

    #[test]
    fn probe_cache_is_refilled_from_database() {
        let dir = temp_dir("probe");
        let store = Store::open_memory().expect("baza");
        let mut catalog = catalog_for(&dir);
        sync_catalog(&store, &mut catalog);

        let film = catalog
            .of_kinds(&[NodeKind::Video])
            .into_iter()
            .find(|node| node.title.contains("Test Film"))
            .expect("film");
        let id = store.item_id(&film.path).unwrap().unwrap();
        let info = rustiio_library::MediaInfo {
            container: "matroska,webm".to_string(),
            duration_ms: Some(7_200_000),
            bitrate_kbps: Some(1_200),
            size_bytes: 0,
            video: Some(rustiio_library::VideoStream {
                codec: "hevc".to_string(),
                width: 1920,
                height: 1080,
                bitrate_kbps: None,
                pix_fmt: Some("yuv420p10le".to_string()),
                profile: Some("Main 10".to_string()),
                level: None,
            }),
            audio: Some(rustiio_library::AudioStream {
                index: 0,
                codec: "ac3".to_string(),
                channels: 6,
                language: None,
                bitrate_kbps: None,
            }),
            audio_streams: Vec::new(),
            embedded_subtitles: 0,
        };
        rustiio_library::store::items::update_media(&store, id, &info, Store::now()).expect("metapodaci");

        // Nova sesija (kao nakon restarta) vidi metapodatke bez ffprobe-a.
        let probe = rustiio_library::MediaProbe::new("ffprobe".to_string(), true);
        assert_eq!(warm_probe_cache(&store, &probe), 1);
        let cached = probe.get(&film.path).expect("metapodaci iz baze");
        assert_eq!(cached.video_codec(), Some("hevc"));
        assert_eq!(cached.audio.as_ref().map(|audio| audio.channels), Some(6));
        assert_eq!(cached.duration_ms, Some(7_200_000));
        // Dubina boje mora preživjeti bazu — o njoj ovisi odluka o transcodeu.
        let video = cached.video.as_ref().expect("video");
        assert_eq!(video.pix_fmt.as_deref(), Some("yuv420p10le"));
        assert_eq!(video.profile.as_deref(), Some("Main 10"));
        assert!(
            rustiio_library::store::items::media_without_bit_depth(&store)
                .expect("upit")
                .is_empty(),
            "nema više zapisa bez dubine boje"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn device_key_prefers_udn_over_user_agent() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-av-client-udn", "uuid:aaaa".parse().unwrap());
        headers.insert(axum::http::header::USER_AGENT, "SEC_HHP_TV".parse().unwrap());
        assert_eq!(device_key(&headers), "uuid:aaaa");

        let mut only_agent = axum::http::HeaderMap::new();
        only_agent.insert(axum::http::header::USER_AGENT, "VLC/3.0".parse().unwrap());
        assert_eq!(device_key(&only_agent), "VLC/3.0");
    }
}
