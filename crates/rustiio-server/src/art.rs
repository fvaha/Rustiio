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
use rustiio_library::Store;
use rustiio_library::store::items;

/// "Ima li objekt poster" — baza + osnovni URL.
pub struct StoreArt {
    store: Arc<Store>,
    base_url: Arc<String>,
}

impl StoreArt {
    pub fn new(store: Arc<Store>, base_url: Arc<String>) -> Self {
        Self { store, base_url }
    }
}

impl ArtLookup for StoreArt {
    fn art_url(&self, item_id: &str) -> Option<String> {
        let id: i64 = item_id.parse().ok()?;
        let (file, _source) = items::poster_of(&self.store, id).ok().flatten()?;
        // Red je bitan: i datoteka i zapis u bazi moraju postojati.
        (!file.is_empty()).then(|| format!("{}/art/{id}", self.base_url))
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
