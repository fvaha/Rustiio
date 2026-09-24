//! Prolaz obogacivanja: poster za svaki video koji ga jos nema.
//!
//! Dijele ga server (pozadinski prolaz + `/api/posters/refresh`) i CLI
//! (`rustiio posters`), pa logika zivi ovdje, a ne u serveru.

use crate::Store;
use crate::store::items;
use crate::store::titles;

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

    // Pravilo: jedna serija — jedna slika. Serijal se u ovoj turi traži samo
    // jednom, a njegove epizode dobiju isti poster (i sezone s njima).
    let mut rijeseni: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (id, path, series, title) in pending {
        summary.last_id = id;
        let serija = series.map(|ime| ime.trim().to_string()).filter(|ime| !ime.is_empty());
        if let Some(ime) = &serija
            && !rijeseni.insert(ime.clone())
        {
            continue;
        }

        // Redoslijed: keš → slika uz datoteku → **zapamćeni ID** → riješi ID pa po
        // ID-u → lanac izvora po naslovu (rezerva kad ID nema ili se ne poklopi).
        let nadjeno = enricher.poster_local(id, &path).or_else(|| {
            let kljuc = titles::scope(serija.as_deref(), id);
            let zapamcen = match titles::get(store, &kljuc) {
                Ok(zapis) => zapis,
                Err(error) => {
                    tracing::warn!(%error, kljuc = %kljuc, "ne mogu procitati zapamceni ID");
                    None
                }
            };
            let zapis = match zapamcen {
                // Zapamćeni ID se provjerava: ako naslov/godina ne potvrde zapis
                // (npr. `Fall 2` se zalijepio na film iz 1970.), briše se i traži
                // se iznova — bolje bez ID-a nego poster tuđeg naslova.
                Some(zapis) => {
                    let naslov_uzorka = serija.clone().unwrap_or_else(|| title.clone());
                    let godina = enricher.year_of_sample(&path);
                    if tmdb_zapis_odgovara(&zapis, &naslov_uzorka, godina) {
                        Some(zapis)
                    } else {
                        tracing::warn!(
                            kljuc = %kljuc,
                            provider_id = %zapis.provider_id,
                            zapis = %zapis.title,
                            zapis_godina = ?zapis.year,
                            naslov = %naslov_uzorka,
                            godina = ?godina,
                            "zapamceni ID se ne poklapa — brisem i trazim iznova"
                        );
                        if let Err(error) = titles::forget(store, &kljuc) {
                            tracing::warn!(%error, kljuc = %kljuc, "ne mogu obrisati zapamceni ID");
                        }
                        None
                    }
                }
                None => {
                    let (naslov, je_serija) = match &serija {
                        Some(ime) => (ime.clone(), true),
                        None => (title.clone(), false),
                    };
                    match enricher.resolve_title(&path, &naslov, je_serija) {
                        Some(novi) => {
                            if let Err(error) = titles::save(store, &kljuc, &novi) {
                                tracing::warn!(%error, kljuc = %kljuc, "ne mogu zapamtiti ID");
                            }
                            tracing::info!(
                                kljuc = %kljuc,
                                provider_id = %novi.provider_id,
                                naslov = %novi.title,
                                godina = ?novi.year,
                                "naslov rijesen u ID"
                            );
                            Some(novi)
                        }
                        None => None,
                    }
                }
            };
            zapis.and_then(|zapis| enricher.poster_by_id(id, &zapis))
        });
        let nadjeno = match nadjeno {
            Some(poster) => Some(poster),
            None => match &serija {
                Some(ime) => enricher.poster_for_series(id, &path, ime),
                None => enricher.poster_for(id, &path),
            },
        };
        match nadjeno {
            Some(poster) => {
                let file = poster.path.file_name().map(|name| name.to_string_lossy().to_string());
                match &serija {
                    Some(ime) => {
                        let broj = items::update_poster_for_series(
                            store,
                            ime,
                            file.as_deref(),
                            poster.source.map(|s| s.as_str()),
                        )?;
                        tracing::info!(series = %ime, epizoda = id, epizoda_broj = broj, poster = ?file, "poster serijala");
                    }
                    None => {
                        items::update_poster(store, id, file.as_deref(), poster.source.map(|s| s.as_str()))?;
                    }
                }
                summary.found += 1;
            }
            None if mark_missing => {
                match &serija {
                    Some(ime) => {
                        items::update_poster_for_series(store, ime, None, Some("none"))?;
                    }
                    None => {
                        items::update_poster(store, id, None, Some("none"))?;
                    }
                }
                summary.missing += 1;
            }
            None => {
                tracing::debug!(id, path = %path.display(), "poster: nema ga (ostaje za sljedeci prolaz)")
            }
        }
    }
    Ok(summary)
}

/// Potvrđuje li naslov i godina zapamćeni zapis?
fn tmdb_zapis_odgovara(zapis: &titles::ResolvedTitle, naslov: &str, godina: Option<u32>) -> bool {
    crate::metadata::tmdb::titles_match(naslov, &zapis.title, godina, zapis.year)
}

/// Očisti postere svih videa i njihove keširane slike, da se dohvate iznova —
/// **po ID-u** (naslov se razriješi jednom, pa poster ide ravno na taj zapis).
/// Zapamćeni ID-evi ostaju: oni su ti koji jamče da slika pripada tom naslovu.
pub fn forget_video_posters(store: &Store, enricher: &Enricher) -> rusqlite::Result<usize> {
    let ids = items::clear_video_posters(store)?;
    for id in &ids {
        enricher.forget(*id);
    }
    Ok(ids.len())
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
