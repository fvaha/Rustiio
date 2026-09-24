//! Prolaz obogacivanja: poster za svaki video koji ga jos nema.
//!
//! Dijele ga server (pozadinski prolaz + `/api/posters/refresh`) i CLI
//! (`rustiio posters`), pa logika zivi ovdje, a ne u serveru.

use crate::Store;
use crate::store::items;

use super::Enricher;

/// Sto je jedan prolaz napravio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PassSummary {
    /// Koliko je objekata pogledano.
    pub processed: usize,
    /// Koliko je postera dohvaceno.
    pub found: usize,
    /// Koliko je oznaceno kao "nema ga" (samo uz `mark_missing`).
    pub missing: usize,
    /// Zadnji obradjeni id — kursor za sljedeci prolaz.
    pub last_id: i64,
}

impl PassSummary {
    pub fn add(&mut self, other: Self) {
        self.processed += other.processed;
        self.found += other.found;
        self.missing += other.missing;
        self.last_id = other.last_id;
    }
}

/// Jedna tura: do `batch` objekata, pocinjuci od `after_id`.
///
/// `mark_missing` odlucuje pise li se "probano, nema ga". Pozadinski prolaz to
/// **ne** radi (bez mreze bi cijela biblioteka bila oznacena zauvijek), a rucni
/// prolaz smije.
pub fn run_pass(
    store: &Store,
    enricher: &Enricher,
    batch: usize,
    after_id: i64,
    mark_missing: bool,
) -> rusqlite::Result<PassSummary> {
    let pending = items::items_needing_poster(store, batch, after_id)?;
    let mut summary = PassSummary { processed: pending.len(), last_id: after_id, ..PassSummary::default() };

    for (id, path) in pending {
        summary.last_id = id;
        match enricher.poster_for(id, &path) {
            Some(poster) => {
                let file = poster.path.file_name().map(|name| name.to_string_lossy().to_string());
                items::update_poster(store, id, file.as_deref(), poster.source.map(|s| s.as_str()))?;
                summary.found += 1;
            }
            None if mark_missing => {
                items::update_poster(store, id, None, Some("none"))?;
                summary.missing += 1;
            }
            None => {
                tracing::debug!(id, path = %path.display(), "poster: nema ga (ostaje za sljedeci prolaz)")
            }
        }
    }
    Ok(summary)
}

/// Prolaz do kraja (u turima od `batch`), najvise `max_batches` tura.
pub fn run_until_done(
    store: &Store,
    enricher: &Enricher,
    batch: usize,
    mark_missing: bool,
    max_batches: usize,
) -> rusqlite::Result<PassSummary> {
    let mut total = PassSummary::default();
    let mut cursor = 0;
    for _ in 0..max_batches {
        let pass = run_pass(store, enricher, batch, cursor, mark_missing)?;
        if pass.processed == 0 {
            break;
        }
        cursor = pass.last_id;
        total.add(pass);
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn cursor_advances_even_when_nothing_is_found() {
        let store = Store::open_memory().expect("baza");
        // `items.root_id` je NOT NULL s referencom na `roots` — pa prvo korijen.
        let conn = store.conn();
        conn.execute("INSERT INTO roots (label, path, kind) VALUES ('X','/x','video')", []).expect("korijen");
        let root_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO items (root_id, path, title, kind) VALUES (?1,'/x/a.mkv','A','video')",
            rusqlite::params![root_id],
        )
        .expect("upis");
        drop(conn);
        // Bez mreze: `poster_for` ne nalazi nista, ali kursor ipak mora napredovati
        // (inace bi prolaz vrtio isti objekt u nedogled).
        let enricher = Enricher::new(None, std::env::temp_dir().join("rustiio-enrich-test"), "ipv4");
        let first = run_pass(&store, &enricher, 10, 0, false).expect("prolaz");
        assert_eq!(first.processed, 1);
        assert!(first.last_id > 0);
        let second = run_pass(&store, &enricher, 10, first.last_id, false).expect("prolaz");
        assert_eq!(second.processed, 0, "isti objekt se ne smije vratiti");
    }
}
