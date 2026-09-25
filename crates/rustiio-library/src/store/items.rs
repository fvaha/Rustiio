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
            parent: parent_path(catalog, node),
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
            series: crate::series::parse(&node.file_name()),
        })
    }
}

/// Putanja roditelja za bazu.
///
/// Kad je u katalogu roditelj izmišljen (serija/sezona koju smo složili), on nema putanju —
/// tada uzimamo pravu mapu u kojoj datoteka leži, da baza ne ostane bez roditelja.
fn parent_path(catalog: &Catalog, node: &Node) -> Option<PathBuf> {
    if let Some(parent) = catalog.get(&node.parent_id) {
        if !parent.path.as_os_str().is_empty() {
            return Some(parent.path.clone());
        }
    }
    node.path.parent().map(|dir| dir.to_path_buf())
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
    /// Ime datoteke postera u kešu (`42.jpg`) ili `None`.
    pub poster: Option<String>,
    /// Odakle poster (`local`, `tmdb`, `wikipedia`, ...).
    pub poster_source: Option<String>,
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
            poster: row.get(9)?,
            poster_source: row.get(10)?,
        })
    }

    /// Kolone moraju odgovarati `from_row` — jedno mjesto za sve upite.
    pub const COLUMNS: &'static str =
        "id, path, title, kind, size, duration_ms, series, season, episode, poster, poster_source";

    /// Koliko kolona `COLUMNS` vraća (dodatne kolone u upitima idu iza njih).
    pub const COLUMN_COUNT: usize = 11;
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

/// Obriši sve što pripada mapama koje više nisu u konfiguraciji.
///
/// `items::sync` briše samo unutar mapa koje se **još skeniraju**, pa redovi
/// izbačene mape (npr. ugašena Muzika, ili stara putanja `/media/…` prije
/// migracije) ostaju zauvijek: pretraga i „Nedavno dodano" vraćaju duhove,
/// a baza raste. Ovo je pometač tih ostataka.
///
/// Prazan `keep` se **preskače** — config bez ijedne mape ne smije obrisati
/// cijelu biblioteku.
pub fn prune_roots(store: &Store, keep: &[i64]) -> rusqlite::Result<usize> {
    if keep.is_empty() {
        return Ok(0);
    }
    let mut conn = store.conn();
    let transaction = conn.transaction()?;
    transaction.execute_batch("CREATE TEMP TABLE _keep (id INTEGER PRIMARY KEY);")?;
    {
        let mut insert = transaction.prepare("INSERT OR IGNORE INTO _keep (id) VALUES (?1)")?;
        for id in keep {
            insert.execute(params![id])?;
        }
    }
    let removed: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM items WHERE root_id NOT IN (SELECT id FROM _keep)",
        [],
        |row| row.get(0),
    )?;
    transaction.execute("DELETE FROM items WHERE root_id NOT IN (SELECT id FROM _keep)", [])?;
    // Povijest gledanja bez objekta nema smisla (id-evi se više neće javiti).
    // `play_state.item_id` inače kaskadno pada uz `items`, ali ovo pokriva i
    // slučaj kad su kaskade isključene.
    transaction.execute("DELETE FROM play_state WHERE item_id NOT IN (SELECT id FROM items)", [])?;
    transaction.execute("DELETE FROM roots WHERE id NOT IN (SELECT id FROM _keep)", [])?;
    transaction.execute_batch("DROP TABLE _keep;")?;
    transaction.commit()?;
    Ok(removed.max(0) as usize)
}

/// Svi objekti zadane vrste (za virtualne poglede i REST).
pub fn by_kind(store: &Store, kind: &str, limit: usize) -> rusqlite::Result<Vec<ItemRow>> {
    let conn = store.conn();
    let sql = format!("SELECT {} FROM items WHERE kind = ?1 ORDER BY title LIMIT ?2", ItemRow::COLUMNS);
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![kind, limit as i64], ItemRow::from_row)?;
    rows.collect()
}

/// Svi objekti (za `Search` s kriterijem `*`), najnoviji prvi.
pub fn recent_all(store: &Store, limit: usize) -> rusqlite::Result<Vec<ItemRow>> {
    let conn = store.conn();
    let sql = format!("SELECT {} FROM items ORDER BY added_at DESC LIMIT ?1", ItemRow::COLUMNS);
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![limit as i64], ItemRow::from_row)?;
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
                audio_codec = ?6, audio_channels = ?7, bitrate = ?8, probed_at = ?9,
                video_pix_fmt = ?10, video_profile = ?11, subtitles = ?12
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
            now,
            info.video.as_ref().and_then(|video| video.pix_fmt.clone()),
            info.video.as_ref().and_then(|video| video.profile.clone()),
            serde_json::to_string(&info.subtitles).ok(),
        ],
    )?;
    Ok(())
}

/// Video zapisi kojima u bazi nema dubine boje (zapisi prije sheme 5).
pub fn media_without_bit_depth(store: &Store) -> rusqlite::Result<Vec<(i64, PathBuf)>> {
    let conn = store.conn();
    let mut statement = conn.prepare(
        "SELECT id, path FROM items
         WHERE video_codec IS NOT NULL
           AND ((video_pix_fmt IS NULL OR video_pix_fmt = '') OR subtitles IS NULL)
         ORDER BY id",
    )?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, PathBuf::from(row.get::<_, String>(1)?))))?;
    rows.collect()
}

/// Metapodaci iz baze, spremni za `MediaProbe` cache nakon restarta.
pub fn media_for_probe(store: &Store) -> rusqlite::Result<Vec<(PathBuf, crate::mediainfo::MediaInfo)>> {
    use crate::mediainfo::{AudioStream, MediaInfo, VideoStream};

    let conn = store.conn();
    let mut statement = conn.prepare(
        "SELECT path, duration_ms, width, height, video_codec, audio_codec, audio_channels, bitrate,
                video_pix_fmt, video_profile, subtitles
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
        let pix_fmt: Option<String> = row.get(8)?;
        let video_profile: Option<String> = row.get(9)?;
        // Ugradjene staze titlova iz baze: bez toga bi topli cache tvrdio da ih nema,
        // pa se TV-u nikad ne bi ponudile (`Language 1` bi bio najbolje sto dobije).
        let subtitles: Vec<crate::subtitles::SubtitleTrack> = row
            .get::<_, Option<String>>(10)?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();

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
                // Dubina boje iz baze: odluka o transcodeu ovisi o njoj.
                pix_fmt,
                profile: video_profile,
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
            embedded_subtitles: subtitles.iter().filter(|staza| staza.path().is_none()).count(),
            subtitles,
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

/// Upiši (ili obriši) poster objekta.
///
/// `poster` je ime datoteke u kešu (`42.jpg`), `source` je izvor
/// (`local`, `tmdb`, `wikipedia`, ...). `None` znači "nema postera".
pub fn update_poster(
    store: &Store,
    item_id: i64,
    poster: Option<&str>,
    source: Option<&str>,
) -> rusqlite::Result<()> {
    let conn = store.conn();
    conn.execute(
        "UPDATE items SET poster = ?2, poster_source = ?3 WHERE id = ?1",
        params![item_id, poster, source],
    )?;
    Ok(())
}

/// Ime datoteke postera u kešu i odakle je došao.
pub fn poster_of(store: &Store, item_id: i64) -> rusqlite::Result<Option<(String, Option<String>)>> {
    let conn = store.conn();
    let mut statement = conn.prepare("SELECT poster, poster_source FROM items WHERE id = ?1")?;
    let mut rows = statement.query(params![item_id])?;
    match rows.next()? {
        Some(row) => Ok(row.get::<_, Option<String>>(0)?.map(|poster| (poster, row.get(1).unwrap_or(None)))),
        None => Ok(None),
    }
}

/// Kandidat za poster: id, putanja, serijal (ako epizoda pripada serijalu) i naslov.
pub type PosterCandidate = (i64, PathBuf, Option<String>, String);

/// Video bez postera — za pozadinsko obogaćivanje (najstariji prvi, da je red stalan).
pub fn items_needing_poster(
    store: &Store,
    limit: usize,
    after_id: i64,
) -> rusqlite::Result<Vec<PosterCandidate>> {
    let conn = store.conn();
    // Kursor po `id` (ne po `added_at`): objekt koji ne uspije ostaje bez postera i
    // bez kursora bi se vracao u svakoj sljedećoj turi — prolaz nikad ne bi zavrsio.
    let mut statement = conn.prepare(
        "SELECT id, path, series, title FROM items
         WHERE kind = 'video' AND poster IS NULL AND poster_source IS NULL AND id > ?2
         ORDER BY id ASC LIMIT ?1",
    )?;
    let rows = statement.query_map(params![limit as i64, after_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            PathBuf::from(row.get::<_, String>(1)?),
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    rows.collect()
}

/// Jedna serija — jedan poster: ista slika za sve njezine epizode.
pub fn update_poster_for_series(
    store: &Store,
    series: &str,
    poster: Option<&str>,
    source: Option<&str>,
) -> rusqlite::Result<usize> {
    let conn = store.conn();
    conn.execute(
        "UPDATE items SET poster = ?2, poster_source = ?3 WHERE series = ?1",
        params![series, poster, source],
    )
}

/// Priprema za ponovno dohvaćanje **po ID-u**: briše postere svih videa (i
/// serijala i filmova) i vraća id-eve čije keširane slike treba izbrisati —
/// bez brisanja datoteke `poster_for` bi vratio staru sliku iz keša.
pub fn clear_video_posters(store: &Store) -> rusqlite::Result<Vec<i64>> {
    let conn = store.conn();
    let ids = {
        let mut statement = conn.prepare(
            "SELECT id FROM items
             WHERE kind = 'video' AND (poster IS NOT NULL OR poster_source IS NOT NULL)",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
        rows.collect::<rusqlite::Result<Vec<i64>>>()?
    };
    conn.execute("UPDATE items SET poster = NULL, poster_source = NULL WHERE kind = 'video'", [])?;
    Ok(ids)
}

/// Zaboravi "probano, nema ga" — sljedeci prolaz ih pokusa ponovno.
pub fn reset_missing_posters(store: &Store) -> rusqlite::Result<usize> {
    let conn = store.conn();
    conn.execute("UPDATE items SET poster_source = NULL WHERE poster_source = 'none'", [])
}

/// Koliko objekata ima poster, koliko ih ceka i koliko je probano bez uspjeha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosterStats {
    pub have: i64,
    pub pending: i64,
    pub none: i64,
}

/// Stanje postera za `/api/posters`.
pub fn poster_stats(store: &Store) -> rusqlite::Result<PosterStats> {
    let conn = store.conn();
    let have =
        conn.query_row("SELECT COUNT(*) FROM items WHERE poster IS NOT NULL AND poster <> ''", [], |row| {
            row.get(0)
        })?;
    let none =
        conn.query_row("SELECT COUNT(*) FROM items WHERE poster_source = 'none'", [], |row| row.get(0))?;
    let pending = conn.query_row(
        "SELECT COUNT(*) FROM items WHERE kind = 'video' AND poster IS NULL AND poster_source IS NULL",
        [],
        |row| row.get(0),
    )?;
    Ok(PosterStats { have, pending, none })
}
