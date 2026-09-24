//! Spajanje skena s bazom: stabilni DLNA id-evi i upis metapodataka.
//!
//! Zašto: DLNA `ObjectID` koji TV zapamti mora značiti isti film i nakon restarta
//! i nakon reskena. Zato id ne dodjeljuje skener (redni broj), nego baza (po putanji).

use std::collections::HashMap;
use std::path::PathBuf;

use rustiio_library::store::items::{self, ScanItem};
use rustiio_library::{Catalog, NodeKind, Store};
use tracing::{info, warn};

/// Što je jedan prolaz promijenio.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncSummary {
    pub roots: usize,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    /// Koliko je čvorova u katalogu dobilo id iz baze.
    pub remapped: usize,
}

/// Upiši katalog u bazu i prebaci id-eve kataloga na id-eve iz baze.
///
/// Idempotentno: drugi poziv nad istim sadržajem ne mijenja id-eve.
pub fn sync_catalog(store: &Store, catalog: &mut Catalog) -> SyncSummary {
    let mut summary = SyncSummary::default();
    let now = Store::now();

    for root in catalog.top_level() {
        if root.path.as_os_str().is_empty() || root.kind != NodeKind::Container {
            continue;
        }
        let label = root.title.clone();
        let path = root.path.to_string_lossy().to_string();
        let kind = "video";
        let root_id = match items::upsert_root(store, &label, &path, kind) {
            Ok(root_id) => root_id,
            Err(error) => {
                warn!(root = %path, error = %error, "mapa nije upisana u bazu");
                continue;
            }
        };

        let batch: Vec<ScanItem> =
            catalog.under(&root.path).iter().filter_map(|node| ScanItem::from_node(node, catalog)).collect();

        match items::sync(store, root_id, &batch, now) {
            Ok(report) => {
                summary.roots += 1;
                summary.added += report.added;
                summary.updated += report.updated;
                summary.removed += report.removed;
            }
            Err(error) => warn!(root = %path, error = %error, "sken nije upisan u bazu"),
        }
    }

    match store.ids_by_path() {
        Ok(rows) => {
            let ids: HashMap<PathBuf, String> =
                rows.into_iter().map(|(path, id)| (PathBuf::from(path), id.to_string())).collect();
            summary.remapped = catalog.remap_ids(&ids);
        }
        Err(error) => warn!(error = %error, "id-evi iz baze nisu procitani"),
    }

    info!(
        roots = summary.roots,
        added = summary.added,
        updated = summary.updated,
        removed = summary.removed,
        remapped = summary.remapped,
        "biblioteka uskladena s bazom"
    );
    summary
}

/// Prefila `MediaProbe` cache iz baze — nakon restarta nema ponovnog ffprobe-a.
pub fn warm_probe_cache(store: &Store, probe: &rustiio_library::MediaProbe) -> usize {
    let rows = match items::media_for_probe(store) {
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
    use rustiio_library::ScanOptions;
    use rustiio_library::scan::scan;

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
    fn sync_writes_library_and_remaps_ids_to_database() {
        let dir = temp_dir("sync");
        let store = Store::open_memory().expect("baza");
        let mut catalog = catalog_for(&dir);

        let first = sync_catalog(&store, &mut catalog);
        assert_eq!(first.roots, 1);
        // 3 mape (Filmovi, Serije, Serije/S01) + 2 videa.
        assert_eq!(first.added, 5);
        assert!(first.remapped >= 5);

        // Film u katalogu sada ima id iz baze.
        let film = catalog
            .of_kinds(&[NodeKind::Video])
            .into_iter()
            .find(|node| node.title.contains("Test Film"))
            .expect("film u katalogu");
        let db_id = store.item_id(&film.path).unwrap().expect("id u bazi");
        assert_eq!(film.id, db_id.to_string(), "DLNA id mora biti id iz baze");

        // Djeca i dalje pokazuju na prave id-eve.
        let root = catalog.top_level().first().cloned().expect("root");
        assert!(!root.children.is_empty());
        for child in &root.children {
            assert!(catalog.get(child).is_some(), "child id {child} postoji u katalogu");
        }

        // Drugi prolaz ne mijenja id-eve.
        let second = sync_catalog(&store, &mut catalog);
        assert_eq!(second.added, 0);
        let film_again = catalog
            .of_kinds(&[NodeKind::Video])
            .into_iter()
            .find(|node| node.title.contains("Test Film"))
            .expect("film");
        assert_eq!(film_again.id, db_id.to_string());

        // Serija je prepoznata iz imena datoteke.
        let series = items::series_list(&store).unwrap();
        assert_eq!(series, vec![("Zlo".to_string(), 1)]);

        let _ = std::fs::remove_dir_all(&dir);
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
                pix_fmt: None,
                profile: None,
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
        items::update_media(&store, id, &info, Store::now()).expect("metapodaci");

        // Nova sesija (kao nakon restarta) vidi metapodatke bez ffprobe-a.
        let probe = rustiio_library::MediaProbe::new("ffprobe".to_string(), true);
        assert_eq!(warm_probe_cache(&store, &probe), 1);
        let cached = probe.get(&film.path).expect("metapodaci iz baze");
        assert_eq!(cached.video_codec(), Some("hevc"));
        assert_eq!(cached.audio.as_ref().map(|audio| audio.channels), Some(6));
        assert_eq!(cached.duration_ms, Some(7_200_000));

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
