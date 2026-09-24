//! Upsert skena: mapa + datoteke u tablicu `items`, bez mijenjanja postojećih id-eva.

use std::path::PathBuf;

use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::scan::{Catalog, Node, NodeKind};
use crate::series::SeriesInfo;

/// Jedan zapis iz skena, spreman za bazu.
#[derive(Debug, Clone)]
pub struct ScanItem {
    pub path: PathBuf,
    pub parent: Option<PathBuf>,
    pub title: String,
    /// "folder" | "video" | "audio" | "image"
    pub kind: String,
    pub ext: String,
    pub size: u64,
    pub mtime: i64,
    pub series: Option<SeriesInfo>,
}

impl ScanItem {
    /// Pretvori čvor iz skenera u zapis za bazu. Titlovi i nepoznato se preskaču —
    /// oni nisu zasebni DLNA objekti (titl se veže na video `res`).
    ///
    /// Roditelj se traži u katalogu (čvor nosi `parent_id`, a baza radi s putanjama).
    pub fn from_node(node: &Node, catalog: &Catalog) -> Option<Self> {
        let kind = match node.kind {
            NodeKind::Container => "folder",
            NodeKind::Video => "video",
            NodeKind::Audio => "audio",
            NodeKind::Image => "image",
            NodeKind::Subtitle | NodeKind::Other => return None,
        };
        Some(Self {
            path: node.path.clone(),
            parent: catalog.get(&node.parent_id).map(|parent| parent.path.clone()),
            title: node.title.clone(),
            kind: kind.to_string(),
            ext: node
                .path
                .extension()
                .map(|extension| extension.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
            size: node.size,
            mtime: node
                .modified
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| since.as_secs() as i64)
                .unwrap_or(0),
            series: crate::series::parse(&node.title),
        })
    }
}

/// Što je sken promijenio.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
}

impl SyncReport {
    pub fn total(&self) -> usize {
        self.added + self.updated
    }
}

/// Redak iz baze (ono što REST/DLNA vraćaju).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRow {
    pub id: i64,
    pub path: PathBuf,
    pub title: String,
    pub kind: String,
    pub size: u64,
    pub duration_ms: Option<i64>,
    pub series: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

impl ItemRow {
    /// Ime datoteke s diska (ono što ide u URL).
    pub fn file_name(&self) -> String {
        self.path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default()
    }

    pub fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            path: PathBuf::from(row.get::<_, String>(1)?),
            title: row.get(2)?,
            kind: row.get(3)?,
            size: row.get::<_, i64>(4)?.max(0) as u64,
            duration_ms: row.get(5)?,
            series: row.get(6)?,
            season: row.get::<_, Option<i64>>(7)?.map(|season| season as u32),
            episode: row.get::<_, Option<i64>>(8)?.map(|episode| episode as u32),
        })
    }

    /// Kolone moraju odgovarati `from_row` — jedno mjesto za sve upite.
    pub const COLUMNS: &'static str = "id, path, title, kind, size, duration_ms, series, season, episode";
}

/// Upisi/popravi mapu (root) i vrati njen id.
pub fn upsert_root(store: &Store, label: &str, path: &str, kind: &str) -> rusqlite::Result<i64> {
    let conn = store.conn();
    conn.execute(
        "INSERT INTO roots (label, path, kind) VALUES (?1, ?2, ?3)
         ON CONFLICT(path) DO UPDATE SET label = excluded.label, kind = excluded.kind",
        params![label, path, kind],
    )?;
    conn.query_row("SELECT id FROM roots WHERE path = ?1", params![path], |row| row.get(0))
}

/// Upiši cijeli sken jedne mape u bazu.
///
/// Postojeći redci se **ažuriraju** (id ostaje), novi se dodaju, a ono što je
/// nestalo s diska se briše (kaskadno i djeca).
pub fn sync(store: &Store, root_id: i64, items: &[ScanItem], now: i64) -> rusqlite::Result<SyncReport> {
    let mut conn = store.conn();
    let transaction = conn.transaction()?;
    let mut report = SyncReport::default();

    // Privremena tablica s putanjama ovog skena — za brisanje bez `IN (...)` liste.
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS _scan (path TEXT PRIMARY KEY);
         DELETE FROM _scan;",
    )?;

    {
        let mut insert_seen = transaction.prepare("INSERT OR REPLACE INTO _scan (path) VALUES (?1)")?;
        for item in items {
            insert_seen.execute([item.path.to_string_lossy().to_string()])?;
        }
    }

    // Prvo mape (pliće putanje), pa datoteke — da `parent_id` uvijek ima metu.
    let mut ordered: Vec<&ScanItem> = items.iter().collect();
    ordered.sort_by_key(|item| item.path.components().count());

    for item in ordered {
        let path = item.path.to_string_lossy().to_string();
        let parent_path = item.parent.as_ref().map(|parent| parent.to_string_lossy().to_string());
        let (series, season, episode) = match &item.series {
            Some(info) => (Some(info.series.clone()), Some(info.season as i64), Some(info.episode as i64)),
            None => (None, None, None),
        };

        let existing: Option<i64> = transaction
            .query_row("SELECT id FROM items WHERE path = ?1", params![path], |row| row.get(0))
            .optional()?;

        match existing {
            Some(id) => {
                transaction.execute(
                    "UPDATE items SET root_id = ?2, parent_id = (SELECT id FROM items WHERE path = ?3),
                            kind = ?4, title = ?5, ext = ?6, size = ?7, mtime = ?8,
                            series = ?9, season = ?10, episode = ?11
                     WHERE id = ?1",
                    params![
                        id,
                        root_id,
                        parent_path,
                        item.kind,
                        item.title,
                        item.ext,
                        item.size as i64,
                        item.mtime,
                        series,
                        season,
                        episode
                    ],
                )?;
                report.updated += 1;
            }
            None => {
                transaction.execute(
                    "INSERT INTO items (root_id, parent_id, path, kind, title, ext, size, mtime, added_at,
                                        series, season, episode)
                     VALUES (?1, (SELECT id FROM items WHERE path = ?2), ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                             ?10, ?11, ?12)",
                    params![
                        root_id,
                        parent_path,
                        path,
                        item.kind,
                        item.title,
                        item.ext,
                        item.size as i64,
                        item.mtime,
                        now,
                        series,
                        season,
                        episode
                    ],
                )?;
                report.added += 1;
            }
        }
    }

    // Broj se računa PRIJE brisanja: kaskada (djeca obrisane mape) se ne broji u
    // `changes()` pojedinačnog DELETE-a.
    let removed: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM items WHERE root_id = ?1 AND path NOT IN (SELECT path FROM _scan)",
        params![root_id],
        |row| row.get(0),
    )?;
    transaction.execute(
        "DELETE FROM items WHERE root_id = ?1 AND path NOT IN (SELECT path FROM _scan)",
        params![root_id],
    )?;
    report.removed = removed.max(0) as usize;
    transaction.execute_batch("DELETE FROM _scan;")?;
    transaction.commit()?;
    Ok(report)
}

/// Svi objekti zadane vrste (za virtualne poglede i REST).
pub fn by_kind(store: &Store, kind: &str, limit: usize) -> rusqlite::Result<Vec<ItemRow>> {
    let conn = store.conn();
    let sql = format!("SELECT {} FROM items WHERE kind = ?1 ORDER BY title LIMIT ?2", ItemRow::COLUMNS);
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![kind, limit as i64], ItemRow::from_row)?;
    rows.collect()
}

/// Djeca jednog objekta (za Browse iz baze).
pub fn children(store: &Store, parent_id: Option<i64>) -> rusqlite::Result<Vec<ItemRow>> {
    let conn = store.conn();
    let sql =
        format!("SELECT {} FROM items WHERE parent_id IS ?1 ORDER BY kind DESC, title", ItemRow::COLUMNS);
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![parent_id], ItemRow::from_row)?;
    rows.collect()
}

/// Zapiši ffprobe metapodatke za objekt.
pub fn update_media(
    store: &Store,
    item_id: i64,
    info: &crate::mediainfo::MediaInfo,
    now: i64,
) -> rusqlite::Result<()> {
    let conn = store.conn();
    conn.execute(
        "UPDATE items SET duration_ms = ?2, width = ?3, height = ?4, video_codec = ?5,
                audio_codec = ?6, audio_channels = ?7, bitrate = ?8, probed_at = ?9
         WHERE id = ?1",
        params![
            item_id,
            info.duration_ms.map(|duration| duration as i64),
            info.video.as_ref().map(|video| video.width as i64),
            info.video.as_ref().map(|video| video.height as i64),
            info.video.as_ref().map(|video| video.codec.clone()),
            info.audio.as_ref().map(|audio| audio.codec.clone()),
            info.audio.as_ref().map(|audio| audio.channels as i64),
            info.bitrate_kbps.map(|bitrate| bitrate as i64),
            now
        ],
    )?;
    Ok(())
}

/// Metapodaci iz baze, spremni za `MediaProbe` cache nakon restarta.
pub fn media_for_probe(store: &Store) -> rusqlite::Result<Vec<(PathBuf, crate::mediainfo::MediaInfo)>> {
    use crate::mediainfo::{AudioStream, MediaInfo, VideoStream};

    let conn = store.conn();
    let mut statement = conn.prepare(
        "SELECT path, duration_ms, width, height, video_codec, audio_codec, audio_channels, bitrate
         FROM items WHERE probed_at IS NOT NULL AND (video_codec IS NOT NULL OR audio_codec IS NOT NULL)",
    )?;
    let rows = statement.query_map([], |row| {
        let path = PathBuf::from(row.get::<_, String>(0)?);
        let duration_ms: Option<i64> = row.get(1)?;
        let width: Option<i64> = row.get(2)?;
        let height: Option<i64> = row.get(3)?;
        let video_codec: Option<String> = row.get(4)?;
        let audio_codec: Option<String> = row.get(5)?;
        let audio_channels: Option<i64> = row.get(6)?;
        let bitrate_kbps: Option<i64> = row.get(7)?;

        let info = MediaInfo {
            // Kontejner se u bazi ne pamti po imenu — za odluku je mjerodavna
            // ekstenzija datoteke, a ne ffprobe `format_name`.
            container: String::new(),
            duration_ms: duration_ms.map(|duration| duration.max(0) as u64),
            bitrate_kbps: bitrate_kbps.map(|bitrate| bitrate.max(0) as u32),
            size_bytes: 0,
            video: video_codec.map(|codec| VideoStream {
                codec,
                width: width.unwrap_or(0).max(0) as u32,
                height: height.unwrap_or(0).max(0) as u32,
                bitrate_kbps: None,
                pix_fmt: None,
                profile: None,
                level: None,
            }),
            audio: audio_codec.map(|codec| AudioStream {
                index: 0,
                codec,
                channels: audio_channels.unwrap_or(2).clamp(0, 255) as u8,
                language: None,
                bitrate_kbps: None,
            }),
            audio_streams: Vec::new(),
            embedded_subtitles: 0,
        };
        Ok((path, info))
    })?;
    rows.collect()
}

/// Serije u biblioteci: naziv + broj epizoda (za virtualno stablo).
pub fn series_list(store: &Store) -> rusqlite::Result<Vec<(String, i64)>> {
    let conn = store.conn();
    let mut statement = conn.prepare(
        "SELECT series, COUNT(*) FROM items WHERE series IS NOT NULL GROUP BY series ORDER BY series",
    )?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}
