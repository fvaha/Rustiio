//! Zapamćeni ID-evi naslova: poster se vuče **po ID-u**, ne po pogađanju naslova.
//!
//! Serijal ima jedan zapis (`series:<ime>`), a samostalni video svoj (`item:<id>`),
//! pa se pretraga po naslovu radi jednom — svaki sljedeći poster tog naslova ide
//! ravno na točan zapis kod izvora.

use rusqlite::{OptionalExtension, params};

use super::Store;

/// Naslov razriješen u kanonski ID kod izvora (npr. TMDB).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTitle {
    pub kind: String,
    pub provider: String,
    pub provider_id: String,
    pub title: String,
    pub year: Option<u32>,
}

/// Ključ po kojem se naslov pamti.
pub fn scope(series: Option<&str>, item_id: i64) -> String {
    match series.map(str::trim).filter(|ime| !ime.is_empty()) {
        Some(ime) => format!("series:{}", ime.to_lowercase()),
        None => format!("item:{item_id}"),
    }
}

/// Zapamćeni ID za taj ključ.
pub fn get(store: &Store, scope: &str) -> rusqlite::Result<Option<ResolvedTitle>> {
    let conn = store.conn();
    conn.query_row(
        "SELECT kind, provider, provider_id, title, year FROM title_ids WHERE scope = ?1",
        params![scope],
        |row| {
            Ok(ResolvedTitle {
                kind: row.get(0)?,
                provider: row.get(1)?,
                provider_id: row.get(2)?,
                title: row.get(3)?,
                year: row.get::<_, Option<i64>>(4)?.map(|godina| godina as u32),
            })
        },
    )
    .optional()
}

/// Zapamti ID (ponovni poziv osvježava zapis).
pub fn save(store: &Store, scope: &str, resolved: &ResolvedTitle) -> rusqlite::Result<()> {
    let conn = store.conn();
    let sada = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|trajanje| trajanje.as_secs() as i64)
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO title_ids (scope, kind, provider, provider_id, title, year, resolved_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(scope) DO UPDATE SET
             kind = excluded.kind,
             provider = excluded.provider,
             provider_id = excluded.provider_id,
             title = excluded.title,
             year = excluded.year,
             resolved_at = excluded.resolved_at",
        params![
            scope,
            resolved.kind,
            resolved.provider,
            resolved.provider_id,
            resolved.title,
            resolved.year.map(|godina| godina as i64),
            sada
        ],
    )?;
    Ok(())
}

/// Zaboravi jedan zapis (kad se pokaže da ID ne pripada tom naslovu).
pub fn forget(store: &Store, scope: &str) -> rusqlite::Result<usize> {
    let conn = store.conn();
    conn.execute("DELETE FROM title_ids WHERE scope = ?1", params![scope])
}

/// Koliko naslova ima zapamćen ID (za sučelje: „po ID-u").
pub fn count(store: &Store) -> rusqlite::Result<i64> {
    let conn = store.conn();
    conn.query_row("SELECT COUNT(*) FROM title_ids", [], |row| row.get(0))
}

/// Zaboravi sve ID-eve (uz ponovno dohvaćanje iz nule).
pub fn forget_all(store: &Store) -> rusqlite::Result<usize> {
    let conn = store.conn();
    conn.execute("DELETE FROM title_ids", [])
}
