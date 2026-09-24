//! Prihvat skena u bazu: jedan prolaz koji upiše katalog i vrati mu stabilne id-eve.
//!
//! Nalazi se u biblioteci (ne u serveru) jer je čista operacija nad `Catalog` + `Store`:
//! skener proizvede stablo, baza mu dodijeli id-eve, i to je sve.

use std::collections::HashMap;
use std::path::PathBuf;

use tracing::{info, warn};

use super::Store;
use super::items::{self, ScanItem};
use crate::scan::{Catalog, NodeKind};

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
pub fn adopt_catalog(store: &Store, catalog: &mut Catalog) -> SyncSummary {
    let mut summary = SyncSummary::default();
    let now = Store::now();

    for root in catalog.top_level() {
        if root.path.as_os_str().is_empty() || root.kind != NodeKind::Container {
            continue;
        }
        let label = root.title.clone();
        let path = root.path.to_string_lossy().to_string();
        let root_id = match items::upsert_root(store, &label, &path, "video") {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScanOptions;
    use crate::scan::scan;
    use rustiio_core::config::{Root, RootKind};

    fn temp_dir(name: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = format!(
            "rustiio-adopt-{}-{}-{}",
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
    fn adopt_remaps_ids_and_keeps_them_on_second_pass() {
        let dir = temp_dir("ids");
        let store = Store::open_memory().expect("baza");
        let mut catalog = catalog_for(&dir);

        let first = adopt_catalog(&store, &mut catalog);
        assert_eq!(first.roots, 1);
        assert_eq!(
            first.added, 3,
            "korijen + film + epizoda: mape se prazne jer epizode idu pod seriju, pa ih nema"
        );
        assert!(first.remapped >= 3);

        let film = catalog
            .of_kinds(&[NodeKind::Video])
            .into_iter()
            .find(|node| node.title.contains("Test Film"))
            .expect("film");
        let db_id = store.item_id(&film.path).unwrap().expect("id u bazi");
        assert_eq!(film.id, db_id.to_string(), "DLNA id dolazi iz baze");

        // Djeca pokazuju na id-eve koji postoje.
        let root = catalog.top_level().first().cloned().expect("root");
        assert!(!root.children.is_empty());
        for child in &root.children {
            assert!(catalog.get(child).is_some(), "child id {child} postoji");
        }

        let second = adopt_catalog(&store, &mut catalog);
        assert_eq!(second.added, 0, "resken ne dodaje nove id-eve");
        let film_again = catalog
            .of_kinds(&[NodeKind::Video])
            .into_iter()
            .find(|node| node.title.contains("Test Film"))
            .expect("film");
        assert_eq!(film_again.id, db_id.to_string());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleted_files_are_removed_from_the_database() {
        let dir = temp_dir("delete");
        let store = Store::open_memory().expect("baza");
        let mut catalog = catalog_for(&dir);
        adopt_catalog(&store, &mut catalog);
        let before = store.item_count().unwrap();

        std::fs::remove_file(dir.join("Test Film (2026).mkv")).expect("brisanje");
        let mut fresh = catalog_for(&dir);
        let summary = adopt_catalog(&store, &mut fresh);
        assert_eq!(summary.removed, 1);
        assert_eq!(store.item_count().unwrap(), before - 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn series_metadata_lands_in_the_database() {
        let dir = temp_dir("series");
        let store = Store::open_memory().expect("baza");
        let mut catalog = catalog_for(&dir);
        adopt_catalog(&store, &mut catalog);

        let episodes = super::items::by_kind(&store, "video", 50).unwrap();
        // Epizoda se u bazi traži po seriji (naslov je sada čitljiv: `S01E03`), a serija
        // dolazi iz imena datoteke — ne iz naslova za prikaz.
        let episode = episodes
            .iter()
            .find(|item| item.series.as_deref() == Some("Zlo"))
            .expect("epizoda sa serijom iz imena datoteke");
        assert_eq!(episode.title, "S01E03", "u bazi je čitljiv naslov");
        assert_eq!(
            (episode.series.as_deref(), episode.season, episode.episode),
            (Some("Zlo"), Some(1), Some(3))
        );
        assert_eq!(super::items::series_list(&store).unwrap(), vec![("Zlo".to_string(), 1)]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
