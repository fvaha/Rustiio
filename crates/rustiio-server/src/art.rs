//! Posteri: DIDL dobiva URL samo ako slika stvarno postoji, i posluživanje slika.
//!
//! Dva posla, oba tanka:
//! 1. [`StoreArt`] odgovara CDS-u "ima li ovaj objekt poster" (CDS ne zna za bazu).
//! 2. [`serve`] vraća datoteku iz keša za `GET /art/{id}`.
//!
//! Slika se **ne** čita u bazu i ne drži u memoriji — TV-i je sami keširaju.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustiio_cds::ArtLookup;
use rustiio_library::store::items;
use rustiio_library::{Catalog, Node, Store};
use tokio::sync::RwLock;

/// "Ima li objekt poster" — baza + osnovni URL (+ katalog za serije i sezone).
pub struct StoreArt {
    store: Arc<Store>,
    base_url: Arc<String>,
    /// Potreban samo za izmišljene čvorove (serija/sezona): oni nemaju svoj red u
    /// bazi, pa poster uzimaju od epizoda ispod sebe.
    catalog: Option<Arc<RwLock<Catalog>>>,
}

impl StoreArt {
    pub fn new(store: Arc<Store>, base_url: Arc<String>) -> Self {
        Self { store, base_url, catalog: None }
    }

    /// Daj katalog — bez njega serije i sezone nemaju poster.
    pub fn with_catalog(mut self, catalog: Arc<RwLock<Catalog>>) -> Self {
        self.catalog = Some(catalog);
        self
    }

    /// Poster iz podstabla izmišljenog čvora.
    ///
    /// Sezone gledamo **od najnovije prema starijoj** (`children` su uzlazno, pa
    /// idemo unatrag): serija na kartici nosi poster sezone koju čovjek gleda —
    /// posljednje. Ako ta sezona nema sliku, ide sljedeća starija, pa dalje.
    fn poster_from_subtree(&self, node: &Node, catalog: &Catalog) -> Option<i64> {
        for child in node.children.iter().rev() {
            if let Ok(id) = child.parse::<i64>() {
                if self.has_art(id) {
                    return Some(id);
                }
                continue;
            }
            // Sezona: isti izbor kao kad se traži poster same sezone — inače bi
            // kartica serije i kartica sezone pokazivale različite slike.
            if let Some(season) = catalog.get(child) {
                if let Some(id) = self.poster_from_season(season) {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Epizoda čiji poster predstavlja sezonu (zadnja koja ga ima).
    fn poster_from_season(&self, season: &Node) -> Option<i64> {
        season.children.iter().rev().find_map(|episode| {
            let id = episode.parse::<i64>().ok()?;
            self.has_art(id).then_some(id)
        })
    }

    fn has_art(&self, id: i64) -> bool {
        matches!(items::poster_of(&self.store, id), Ok(Some((file, _))) if !file.is_empty())
    }
}

impl ArtLookup for StoreArt {
    fn art_url(&self, item_id: &str) -> Option<String> {
        if let Ok(id) = item_id.parse::<i64>() {
            // Red je bitan: i datoteka i zapis u bazi moraju postojati.
            return self.has_art(id).then(|| format!("{}/art/{id}", self.base_url));
        }
        // Serija (`s:slug`) i sezona (`s:slug:2`) nemaju svoj red — poster ide od
        // epizoda. `try_read` namjerno: ako netko upravo piše katalog, bolje bez
        // postera nego čekanje u async putu.
        let catalog = self.catalog.as_ref()?.try_read().ok()?;
        let node = catalog.get(item_id)?;
        let epizoda = self.poster_from_subtree(node, &catalog)?;
        Some(format!("{}/art/{epizoda}", self.base_url))
    }
}

/// Vrsta slike iz nastavka imena (bez čitanja sadržaja — jeftino i točno).
pub fn content_type(file_name: &str) -> &'static str {
    match file_name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
}

/// Ime datoteke iz baze — **nikad** putanja.
///
/// Baza pamti samo ime (`42.jpg`). Ako bi nekim čudom tamo došla putanja
/// (`../../etc/passwd`), ovdje se svodi na zadnji dio i ne izlazi iz keša.
pub fn poster_file_name(file: &str) -> Option<String> {
    if file.is_empty() {
        return None;
    }
    Path::new(file).file_name()?.to_str().map(|name| name.to_string())
}

/// Poster za `GET /art/{id}`: ime iz baze + provjera da datoteka postoji u kešu.
pub fn serve(store: &Store, art_dir: &Path, item_id: i64) -> Option<(PathBuf, &'static str)> {
    let (file, _source) = items::poster_of(store, item_id).ok().flatten()?;
    let name = poster_file_name(&file)?;
    let path = art_dir.join(name);
    path.is_file().then(|| (path, content_type(&file)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_follows_extension() {
        assert_eq!(content_type("42.jpg"), "image/jpeg");
        assert_eq!(content_type("42.JPEG"), "image/jpeg");
        assert_eq!(content_type("42.png"), "image/png");
        assert_eq!(content_type("42.webp"), "image/webp");
        assert_eq!(content_type("bez-nastavka"), "image/jpeg");
    }

    #[test]
    fn poster_name_never_leaves_the_cache() {
        assert_eq!(poster_file_name("42.jpg").as_deref(), Some("42.jpg"));
        // Putanja iz baze svodi se na zadnji dio — ne izlazi iz mape keša.
        assert_eq!(poster_file_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(poster_file_name("/tmp/x/99.png").as_deref(), Some("99.png"));
        assert_eq!(poster_file_name(""), None);
    }
}
