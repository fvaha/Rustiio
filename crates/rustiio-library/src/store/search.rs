//! Pretraga preko FTS5 (isti indeks koristi i DLNA `Search` i web UI).

use rusqlite::params;

use super::Store;
use super::items::ItemRow;

/// Pogodak pretrage (+ rang; manji je bolji).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub item: ItemRow,
    pub rank: f64,
}

/// Pretvori korisnički upit u FTS5 izraz.
///
/// Naslov se traži po **prefiksu** (`sic` → `Sicario`), a putanja po **cijeloj riječi**
/// — inače "film" povuče cijelu mapu `/media/filmovi` kao pogodak.
/// Svaki token se veže s AND. Navodnici se dupliraju da upit s `"` ili `(` ne sruši parser.
pub fn to_match_query(query: &str) -> String {
    let tokens: Vec<String> = query
        .split_whitespace()
        .map(|token| token.trim_matches(|c: char| !c.is_alphanumeric() && !"_-".contains(c)))
        .filter(|token| !token.is_empty())
        .map(|token| {
            let token = token.replace('"', "\"\"");
            format!("(title:\"{token}\"* OR path:\"{token}\")")
        })
        .collect();
    tokens.join(" AND ")
}

/// Pretraži naslov i putanju. Prazan upit vraća prazno (ne cijelu biblioteku).
pub fn search(store: &Store, query: &str, limit: usize) -> rusqlite::Result<Vec<SearchHit>> {
    let match_query = to_match_query(query);
    if match_query.is_empty() {
        return Ok(Vec::new());
    }

    let conn = store.conn();
    let sql = format!(
        "SELECT {}, bm25(items_fts) AS rank FROM items_fts f JOIN items i ON i.id = f.rowid
         WHERE items_fts MATCH ?1 ORDER BY bm25(items_fts), i.title LIMIT ?2",
        ItemRow::COLUMNS.split(", ").map(|column| format!("i.{column}")).collect::<Vec<_>>().join(", ")
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![match_query, limit as i64], |row| {
        Ok(SearchHit { item: ItemRow::from_row(row)?, rank: row.get::<_, f64>(9)? })
    })?;
    rows.collect()
}

/// Pretraga ograničena na vrstu sadržaja ("video", "audio", "image").
pub fn search_kind(store: &Store, query: &str, kind: &str, limit: usize) -> rusqlite::Result<Vec<SearchHit>> {
    Ok(search(store, query, limit * 4)?.into_iter().filter(|hit| hit.item.kind == kind).take(limit).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::items::{self, ScanItem};
    use std::path::PathBuf;

    fn store_with_library() -> Store {
        let store = Store::open_memory().expect("baza");
        let root_id = items::upsert_root(&store, "Filmovi", "/media/filmovi", "video").expect("root");
        let entries = [
            ("/media/filmovi/Test Film (2026).mkv", "Test Film (2026)", "video"),
            ("/media/filmovi/Žestoki Dečki.mkv", "Žestoki Dečki", "video"),
            ("/media/filmovi/Sicario.mkv", "Sicario", "video"),
            ("/media/muzika/Test Pjesma.mp3", "Test Pjesma", "audio"),
        ];
        let items: Vec<ScanItem> = entries
            .iter()
            .map(|(path, title, kind)| ScanItem {
                path: PathBuf::from(path),
                parent: None,
                title: title.to_string(),
                kind: kind.to_string(),
                ext: path.rsplit('.').next().unwrap_or_default().to_string(),
                size: 1,
                mtime: 0,
                series: None,
            })
            .collect();
        items::sync(&store, root_id, &items, Store::now()).expect("sken");
        store
    }

    #[test]
    fn search_finds_by_word_and_prefix() {
        let store = store_with_library();
        let hits = search(&store, "film", 10).unwrap();
        assert_eq!(hits.len(), 1, "samo 'Test Film', ne i cijela mapa /media/filmovi");
        assert_eq!(hits[0].item.title, "Test Film (2026)");

        // Prefiks: "tes" mora naći i film i pjesmu.
        assert_eq!(search(&store, "tes", 10).unwrap().len(), 2);
        // Prefiks na naslovu: "sica" → "Sicario".
        assert_eq!(search(&store, "sica", 10).unwrap().len(), 1);
    }

    #[test]
    fn match_query_uses_prefix_for_title_and_exact_word_for_path() {
        assert_eq!(to_match_query("film"), "(title:\"film\"* OR path:\"film\")");
        assert_eq!(
            to_match_query("test film"),
            "(title:\"test\"* OR path:\"test\") AND (title:\"film\"* OR path:\"film\")"
        );
        // Navodnik ne smije razbiti izraz.
        assert!(to_match_query("\"film").contains("film"));
    }

    #[test]
    fn multiple_words_are_combined_with_and() {
        let store = store_with_library();
        assert_eq!(search(&store, "test film", 10).unwrap().len(), 1);
        // "test sicario" ne postoji ni u jednom naslovu.
        assert!(search(&store, "test sicario", 10).unwrap().is_empty());
    }

    #[test]
    fn diacritics_are_ignored() {
        let store = store_with_library();
        // Bez kvačica mora naći "Žestoki Dečki".
        assert_eq!(search(&store, "zestoki", 10).unwrap().len(), 1);
        assert_eq!(search(&store, "decki", 10).unwrap().len(), 1);
    }

    #[test]
    fn kind_filter_separates_music_from_video() {
        let store = store_with_library();
        assert_eq!(search_kind(&store, "test", "audio", 10).unwrap().len(), 1);
        assert_eq!(search_kind(&store, "test", "video", 10).unwrap().len(), 1);
    }

    #[test]
    fn broken_quotes_do_not_crash_the_query() {
        let store = store_with_library();
        assert!(search(&store, "(\"film", 10).is_ok());
        assert!(search(&store, "   ", 10).unwrap().is_empty());
        assert!(search(&store, "NEAR(", 10).is_ok());
    }

    #[test]
    fn path_is_searchable_too() {
        let store = store_with_library();
        assert_eq!(search(&store, "muzika", 10).unwrap().len(), 1);
    }
}
