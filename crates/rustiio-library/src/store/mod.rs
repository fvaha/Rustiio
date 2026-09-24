//! SQLite indeks biblioteke: stabilni id-evi, FTS pretraga, watch-state.
//!
//! Zašto baza: DLNA `ObjectID` mora preživjeti restart i resken (TV pamti id-eve
//! u svojim playlistama), a "nastavi gledati" traži da pozicija bude vezana na
//! stabilan id, ne na redni broj iz posljednjeg skena.

pub mod adopt;
pub mod items;
pub mod play_state;
pub mod schema;
pub mod search;
pub mod titles;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

pub use adopt::{SyncSummary, adopt_catalog};
pub use items::{ItemRow, ScanItem, SyncReport};
pub use play_state::Position;
pub use search::SearchHit;

/// Ručka na bazu. Jedna konekcija + `Mutex`: SQLite je dovoljno brz za naš promet,
/// a WAL čini da čitanje tijekom skena ne čeka.
#[derive(Debug)]
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Otvori bazu na putanji (mapa se stvara ako ne postoji).
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        schema::migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Baza u memoriji — za testove i za `--no-scan` probe.
    pub fn open_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        schema::migrate(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Zaključaj bazu. Otrovani mutex ne ruši server — uzimamo konekciju natrag.
    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Verzija sheme u bazi (za `/api/status` i migracije).
    pub fn schema_version(&self) -> rusqlite::Result<i32> {
        self.conn().query_row("PRAGMA user_version", [], |row| row.get(0))
    }

    /// Broj redaka po vrsti sadržaja ("folder", "video", ...).
    pub fn counts(&self) -> rusqlite::Result<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut statement = conn.prepare("SELECT kind, COUNT(*) FROM items GROUP BY kind")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// Ukupan broj objekata u bazi.
    pub fn item_count(&self) -> rusqlite::Result<i64> {
        self.conn().query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
    }

    /// Id za putanju (stabilan kroz reskenove).
    pub fn item_id(&self, path: &Path) -> rusqlite::Result<Option<i64>> {
        let conn = self.conn();
        let mut statement = conn.prepare("SELECT id FROM items WHERE path = ?1")?;
        let path = path.to_string_lossy().to_string();
        let mut rows = statement.query([path])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Sve putanje → id (jedan upit; koristi ga skener da DLNA id-evi budu stabilni).
    pub fn ids_by_path(&self) -> rusqlite::Result<Vec<(String, i64)>> {
        let conn = self.conn();
        let mut statement = conn.prepare("SELECT path, id FROM items")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// Sekunde od epohe (jedini format vremena u bazi).
    pub fn now() -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs() as i64).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::series::SeriesInfo;

    fn item(path: &str, parent: Option<&str>, title: &str, kind: &str) -> ScanItem {
        ScanItem {
            path: path.into(),
            parent: parent.map(Into::into),
            title: title.to_string(),
            kind: kind.to_string(),
            ext: "mkv".to_string(),
            size: 1024,
            mtime: 1_700_000_000,
            series: None,
        }
    }

    fn store_with_root() -> (Store, i64) {
        let store = Store::open_memory().expect("baza");
        let root_id = items::upsert_root(&store, "Filmovi", "/media/filmovi", "video").expect("root");
        (store, root_id)
    }

    #[test]
    fn sync_assigns_ids_and_survives_rescan() {
        let (store, root_id) = store_with_root();
        let scan = vec![
            item("/media/filmovi", None, "Filmovi", "folder"),
            item("/media/filmovi/Test Film (2026).mkv", Some("/media/filmovi"), "Test Film (2026)", "video"),
        ];
        let first = items::sync(&store, root_id, &scan, Store::now()).expect("prvi sken");
        assert_eq!(first.added, 2);

        let id_before = store.item_id(Path::new("/media/filmovi/Test Film (2026).mkv")).unwrap().unwrap();

        // Ponovni sken istih fajlova: nema novih id-eva, id ostaje isti.
        let again = items::sync(&store, root_id, &scan, Store::now()).expect("drugi sken");
        assert_eq!(again.added, 0);
        assert_eq!(again.updated, 2);
        let id_after = store.item_id(Path::new("/media/filmovi/Test Film (2026).mkv")).unwrap().unwrap();
        assert_eq!(id_before, id_after, "id se ne smije mijenjati pri reskenu");
    }

    #[test]
    fn sync_removes_deleted_files_and_their_children() {
        let (store, root_id) = store_with_root();
        let scan = vec![
            item("/media/filmovi", None, "Filmovi", "folder"),
            item("/media/filmovi/Serije", Some("/media/filmovi"), "Serije", "folder"),
            item("/media/filmovi/Serije/S01E01.mkv", Some("/media/filmovi/Serije"), "S01E01", "video"),
        ];
        items::sync(&store, root_id, &scan, Store::now()).expect("sken");
        assert_eq!(store.item_count().unwrap(), 3);

        // Serije su obrisane s diska.
        let smaller = vec![item("/media/filmovi", None, "Filmovi", "folder")];
        let report = items::sync(&store, root_id, &smaller, Store::now()).expect("sken nakon brisanja");
        assert_eq!(report.removed, 2, "obrisana mapa i epizoda");
        assert_eq!(store.item_count().unwrap(), 1);
    }

    #[test]
    fn counts_and_schema_version_are_reported() {
        let (store, root_id) = store_with_root();
        let scan = vec![
            item("/media/filmovi/A.mkv", None, "A", "video"),
            item("/media/filmovi/B.mkv", None, "B", "video"),
            item("/media/filmovi/C.mp3", None, "C", "audio"),
        ];
        items::sync(&store, root_id, &scan, Store::now()).expect("sken");
        assert_eq!(store.schema_version().unwrap(), schema::VERSION);
        let counts = store.counts().unwrap();
        assert!(counts.contains(&("video".to_string(), 2)));
        assert!(counts.contains(&("audio".to_string(), 1)));
    }

    #[test]
    fn children_follow_parent_links() {
        let (store, root_id) = store_with_root();
        let scan = vec![
            item("/media/filmovi", None, "Filmovi", "folder"),
            item("/media/filmovi/Test Film (2026).mkv", Some("/media/filmovi"), "Test Film (2026)", "video"),
        ];
        items::sync(&store, root_id, &scan, Store::now()).expect("sken");
        let folder_id = store.item_id(Path::new("/media/filmovi")).unwrap().unwrap();
        let children = items::children(&store, Some(folder_id)).unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].title, "Test Film (2026)");
    }

    #[test]
    fn series_metadata_is_stored_and_listed() {
        let (store, root_id) = store_with_root();
        let mut episode = item("/media/serije/Zlo/S01E03.mkv", None, "Zlo S01E03", "video");
        episode.series = Some(SeriesInfo { series: "Zlo".to_string(), season: 1, episode: 3 });
        items::sync(&store, root_id, &[episode], Store::now()).expect("sken");

        let found = items::by_kind(&store, "video", 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].series.as_deref(), Some("Zlo"));
        assert_eq!(found[0].season, Some(1));
        assert_eq!(found[0].episode, Some(3));
        assert_eq!(items::series_list(&store).unwrap(), vec![("Zlo".to_string(), 1)]);
    }
}
