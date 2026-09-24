//! Watch-state: gdje je koji uređaj stao. Osnova za "Nastavi gledati".
//!
//! Ključ uređaja: UDN iz DLNA zaglavlja ako ga ima, inače `User-Agent`. Tako
//! Samsung i telefon imaju odvojene pozicije za isti film.

use rusqlite::params;

use super::Store;
use super::items::ItemRow;

/// Pozicija jednog uređaja na jednom objektu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub item_id: i64,
    pub position_ms: i64,
    pub duration_ms: Option<i64>,
    pub played: bool,
    pub updated_at: i64,
}

impl Position {
    /// 0.0–1.0 (za progress bar u web UI-ju).
    pub fn progress(&self) -> f64 {
        match self.duration_ms {
            Some(duration) if duration > 0 => (self.position_ms as f64 / duration as f64).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

/// Prag ispod kojeg se pozicija ne pamti (10 s) — inače svaki otvoreni film
/// završi u "nastavi gledati".
pub const MIN_RESUME_MS: i64 = 10_000;

/// Zapiši poziciju (ili je obriši ako je film gotov).
pub fn set(
    store: &Store,
    device: &str,
    item_id: i64,
    position_ms: i64,
    duration_ms: Option<i64>,
    now: i64,
) -> rusqlite::Result<()> {
    let played = match duration_ms {
        // Zadnjih 5% (i barem 30 s prije kraja) = odgledano.
        Some(duration) if duration > 0 => {
            position_ms >= duration - 30_000 && position_ms as f64 >= duration as f64 * 0.95
        }
        _ => false,
    };

    if position_ms < MIN_RESUME_MS && !played {
        return clear(store, device, item_id);
    }

    let position_ms = match duration_ms {
        Some(duration) if duration > 0 => position_ms.clamp(0, duration),
        _ => position_ms.max(0),
    };

    store.conn().execute(
        "INSERT INTO play_state (device, item_id, position_ms, duration_ms, played, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(device, item_id) DO UPDATE SET
            position_ms = excluded.position_ms,
            duration_ms = excluded.duration_ms,
            played = excluded.played,
            updated_at = excluded.updated_at",
        params![device, item_id, position_ms, duration_ms, played as i64, now],
    )?;
    Ok(())
}

/// Obriši poziciju (npr. "ne nastavljaj").
pub fn clear(store: &Store, device: &str, item_id: i64) -> rusqlite::Result<()> {
    store
        .conn()
        .execute("DELETE FROM play_state WHERE device = ?1 AND item_id = ?2", params![device, item_id])?;
    Ok(())
}

/// Pozicija za jedan objekt.
pub fn get(store: &Store, device: &str, item_id: i64) -> rusqlite::Result<Option<Position>> {
    let conn = store.conn();
    let mut statement = conn.prepare(
        "SELECT item_id, position_ms, duration_ms, played, updated_at FROM play_state
         WHERE device = ?1 AND item_id = ?2",
    )?;
    let mut rows = statement.query(params![device, item_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(Position {
            item_id: row.get(0)?,
            position_ms: row.get(1)?,
            duration_ms: row.get(2)?,
            played: row.get::<_, i64>(3)? != 0,
            updated_at: row.get(4)?,
        })),
        None => Ok(None),
    }
}

/// "Nastavi gledati": nezavršeni filmovi/serije, najnoviji prvi.
pub fn continue_watching(
    store: &Store,
    device: &str,
    limit: usize,
) -> rusqlite::Result<Vec<(ItemRow, Position)>> {
    let conn = store.conn();
    let sql = format!(
        "SELECT {}, p.position_ms, p.duration_ms, p.played, p.updated_at
         FROM play_state p JOIN items i ON i.id = p.item_id
         WHERE p.device = ?1 AND p.played = 0 AND p.position_ms >= ?2 AND i.kind = 'video'
         ORDER BY p.updated_at DESC LIMIT ?3",
        ItemRow::COLUMNS.split(", ").map(|column| format!("i.{column}")).collect::<Vec<_>>().join(", ")
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![device, MIN_RESUME_MS, limit as i64], |row| {
        let item = ItemRow::from_row(row)?;
        let position = Position {
            item_id: item.id,
            position_ms: row.get(ItemRow::COLUMN_COUNT)?,
            duration_ms: row.get(ItemRow::COLUMN_COUNT + 1)?,
            played: row.get::<_, i64>(11)? != 0,
            updated_at: row.get(12)?,
        };
        Ok((item, position))
    })?;
    rows.collect()
}

/// Označi kao odgledano (kad TV javi kraj).
pub fn mark_played(store: &Store, device: &str, item_id: i64, now: i64) -> rusqlite::Result<()> {
    store.conn().execute(
        "INSERT INTO play_state (device, item_id, position_ms, duration_ms, played, updated_at)
         VALUES (?1, ?2, 0, (SELECT duration_ms FROM items WHERE id = ?2), 1, ?3)
         ON CONFLICT(device, item_id) DO UPDATE SET played = 1, updated_at = excluded.updated_at",
        params![device, item_id, now],
    )?;
    Ok(())
}

/// Broj zapisa po uređaju (za `/api/devices`).
pub fn device_summary(store: &Store) -> rusqlite::Result<Vec<(String, i64)>> {
    let conn = store.conn();
    let mut statement = conn
        .prepare("SELECT device, COUNT(*) FROM play_state GROUP BY device ORDER BY MAX(updated_at) DESC")?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::items::{self, ScanItem};
    use std::path::PathBuf;

    fn store_with_film() -> (Store, i64) {
        let store = Store::open_memory().expect("baza");
        let root_id = items::upsert_root(&store, "Filmovi", "/media/filmovi", "video").expect("root");
        let film = ScanItem {
            path: PathBuf::from("/media/filmovi/Test Film (2026).mkv"),
            parent: None,
            title: "Test Film (2026)".to_string(),
            kind: "video".to_string(),
            ext: "mkv".to_string(),
            size: 1024,
            mtime: 0,
            series: None,
        };
        items::sync(&store, root_id, &[film], Store::now()).expect("sken");
        let id = store.item_id(std::path::Path::new("/media/filmovi/Test Film (2026).mkv")).unwrap().unwrap();
        (store, id)
    }

    #[test]
    fn position_is_remembered_after_ten_seconds() {
        let (store, id) = store_with_film();
        set(&store, "tv-samsung", id, 120_000, Some(7_200_000), Store::now()).expect("zapis");
        let position = get(&store, "tv-samsung", id).unwrap().expect("pozicija");
        assert_eq!(position.position_ms, 120_000);
        assert!(!position.played);
        assert!((position.progress() - 0.0166).abs() < 0.01);
    }

    #[test]
    fn short_viewing_does_not_leave_a_trace() {
        let (store, id) = store_with_film();
        set(&store, "tv-samsung", id, 4_000, Some(7_200_000), Store::now()).expect("zapis");
        assert!(get(&store, "tv-samsung", id).unwrap().is_none(), "5 s gledanja nije 'nastavi gledati'");
    }

    #[test]
    fn devices_keep_separate_positions() {
        let (store, id) = store_with_film();
        set(&store, "tv-samsung", id, 300_000, Some(7_200_000), Store::now()).unwrap();
        set(&store, "phone-vlc", id, 600_000, Some(7_200_000), Store::now()).unwrap();
        assert_eq!(get(&store, "tv-samsung", id).unwrap().unwrap().position_ms, 300_000);
        assert_eq!(get(&store, "phone-vlc", id).unwrap().unwrap().position_ms, 600_000);
    }

    #[test]
    fn finished_film_is_marked_played_and_leaves_continue_watching() {
        let (store, id) = store_with_film();
        set(&store, "tv-samsung", id, 300_000, Some(7_200_000), Store::now()).unwrap();
        assert_eq!(continue_watching(&store, "tv-samsung", 10).unwrap().len(), 1);

        set(&store, "tv-samsung", id, 7_180_000, Some(7_200_000), Store::now()).unwrap();
        let position = get(&store, "tv-samsung", id).unwrap().unwrap();
        assert!(position.played, "kraj filma");
        assert!(continue_watching(&store, "tv-samsung", 10).unwrap().is_empty());
    }

    #[test]
    fn clearing_position_removes_it() {
        let (store, id) = store_with_film();
        set(&store, "tv-samsung", id, 300_000, Some(7_200_000), Store::now()).unwrap();
        clear(&store, "tv-samsung", id).unwrap();
        assert!(get(&store, "tv-samsung", id).unwrap().is_none());
    }
}
