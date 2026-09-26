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
    /// Za verziju u URL-u (`?v=`) — bez toga preglednik drži staru sliku pod istim
    /// `/art/<id>` i u dashboardu se vidi poster koji je odavno zamijenjen.
    art_dir: Option<PathBuf>,
    /// Potreban samo za izmišljene čvorove (serija/sezona): oni nemaju svoj red u
    /// bazi, pa poster uzimaju od epizoda ispod sebe.
    catalog: Option<Arc<RwLock<Catalog>>>,
}

impl StoreArt {
    pub fn new(store: Arc<Store>, base_url: Arc<String>) -> Self {
        Self { store, base_url, catalog: None, art_dir: None }
    }

    /// Daj mapu keša slika — bez nje URL nema verziju (preglednik kešira zauvijek).
    pub fn with_art_dir(mut self, art_dir: PathBuf) -> Self {
        self.art_dir = Some(art_dir);
        self
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
        // Video ima poster iz baze; ako ga nema, nastavljamo na mapu/seriju nize.
        if let Ok(id) = item_id.parse::<i64>() {
            if self.has_art(id) {
                return Some(self.url_for(id));
            }
        }
        // Serija (`s:slug`) i sezona (`s:slug:2`) nemaju svoj red — poster ide od
        // epizoda. `try_read` namjerno: ako netko upravo piše katalog, bolje bez
        // postera nego čekanje u async putu.
        let catalog = self.catalog.as_ref()?.try_read().ok()?;
        let node = catalog.get(item_id)?;
        // Mapa sa svojom slikom (`folder.jpg`) ima prednost, pa zadana slika korijena.
        if let Some(url) = self.mapa_slika(node) {
            return Some(url);
        }
        // Nema vlastite slike: kolaz od postera sadrzaja (Nova Player stil).
        if let Some(url) = self.kolaz_url(node, &catalog) {
            return Some(url);
        }
        if let Some(url) = self.zadana_slika(node) {
            return Some(url);
        }
        let epizoda = self.poster_from_subtree(node, &catalog)?;
        Some(self.url_for(epizoda))
    }
}

impl StoreArt {
    /// URL slike s verzijom (`?v=<vrijeme><velicina>`), da svaka promjena postera
    /// dobije novi URL i preglednik/televizor ne prikazuju staru sliku.
    fn url_for(&self, id: i64) -> String {
        match self.version(id) {
            Some(verzija) => format!("{}/art/{id}?v={verzija}", self.base_url),
            None => format!("{}/art/{id}", self.base_url),
        }
    }

    /// Verzija slike: vrijeme promjene i veličina datoteke u kešu.
    fn version(&self, id: i64) -> Option<String> {
        let (file, _source) = items::poster_of(&self.store, id).ok().flatten()?;
        let name = poster_file_name(&file)?;
        let meta = std::fs::metadata(self.art_dir.as_ref()?.join(name)).ok()?;
        let sekunde = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        Some(format!("{sekunde:x}{:x}", meta.len()))
    }
}

/// Verzija slike za `GET /art/{id}` (ETag i `?v=` u URL-u).
pub fn version_of(store: &Store, art_dir: &Path, item_id: i64) -> Option<String> {
    let (file, _source) = items::poster_of(store, item_id).ok().flatten()?;
    let name = poster_file_name(&file)?;
    let meta = std::fs::metadata(art_dir.join(name)).ok()?;
    let sekunde = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(format!("{sekunde:x}{:x}", meta.len()))
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

impl StoreArt {
    /// Slika mape: `folder.jpg` (i srodna imena) u samoj mapi, a za korijene
    /// bez slike — ugradjena zadana slika (Filmovi / Serije).
    fn mapa_slika(&self, node: &Node) -> Option<String> {
        if !node.is_container() {
            return None;
        }
        for ime in IMENA_MAPNE_SLIKE {
            let putanja = node.path.join(ime);
            if putanja.is_file() {
                let v = std::fs::metadata(&putanja)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                return Some(format!("/folderart/{}?v={v}", node.id));
            }
        }
        None
    }

    /// Ugradjena zadana slika za korijene bez ikakvog sadrzaja (prazni Filmovi/Serije).
    fn zadana_slika(&self, node: &Node) -> Option<String> {
        if !node.is_container() || node.parent_id != "0" {
            return None;
        }
        let naziv =
            node.path.file_name().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if naziv.contains("film") || naziv.contains("movie") {
            return Some("/folder-movies.png".to_string());
        }
        if naziv.contains("serij") || naziv.contains("tv") || naziv.contains("show") {
            return Some("/folder-tv.png".to_string());
        }
        None
    }

    /// Do cetiri postera iz podstabla (za kolaz) — isti izbor kao `poster_from_subtree`.
    pub fn posteri_iz_podstabla(&self, node: &Node, catalog: &Catalog, koliko: usize) -> Vec<i64> {
        let mut nadjeni: Vec<i64> = Vec::new();
        // Filmovi su svaki za sebe, a svaka serija (sezona) daje TOCNO JEDAN poster —
        // inace bi kolaz za "Serije" bio cetiri epizode iste serije.
        for child in node.children.iter().rev() {
            if nadjeni.len() >= koliko {
                break;
            }
            if let Ok(id) = child.parse::<i64>() {
                if self.has_art(id) && !nadjeni.contains(&id) {
                    nadjeni.push(id);
                }
                continue;
            }
            let Some(pod) = catalog.get(child) else {
                continue;
            };
            let mut iz_serije: Vec<i64> = Vec::new();
            self.skupi_postiere(pod, catalog, 1, 0, &mut iz_serije);
            if let Some(id) = iz_serije.first()
                && !nadjeni.contains(id)
            {
                nadjeni.push(*id);
            }
        }
        nadjeni
    }

    /// Skuplja postere u dubinu: filmovi su odmah u mapi, a serije idu
    /// serija -> sezona -> epizoda (zato rekurzija, inace "Serije" ostane bez slike).
    fn skupi_postiere(
        &self,
        node: &Node,
        catalog: &Catalog,
        koliko: usize,
        dubina: u8,
        nadjeni: &mut Vec<i64>,
    ) {
        if dubina > 3 || nadjeni.len() >= koliko {
            return;
        }
        for child in node.children.iter().rev() {
            if nadjeni.len() >= koliko {
                return;
            }
            if let Ok(id) = child.parse::<i64>() {
                if self.has_art(id) && !nadjeni.contains(&id) {
                    nadjeni.push(id);
                }
                continue;
            }
            let Some(pod) = catalog.get(child) else {
                continue;
            };
            if let Some(id) = self.poster_from_season(pod)
                && !nadjeni.contains(&id)
            {
                nadjeni.push(id);
            }
            self.skupi_postiere(pod, catalog, koliko, dubina + 1, nadjeni);
        }
    }

    /// Kolaz za mapu: `/collage/{id}` kad ima barem dva postera, inace sam poster.
    fn kolaz_url(&self, node: &Node, catalog: &Catalog) -> Option<String> {
        // Kolaz dobivaju samo korijeni (Filmovi, Serije) — serija i sezona i dalje
        // pokazuju svoj poster, a mapa s djecom svoj jedan poster.
        if node.parent_id != "0" {
            return None;
        }
        let posteri = self.posteri_iz_podstabla(node, catalog, 4);
        match posteri.len() {
            0 => None,
            1 => Some(self.url_for(posteri[0])),
            _ => Some(format!("/collage/{}", node.id)),
        }
    }

    /// Gdje stoji (predmemorirani) kolaz za pojedinu mapu.
    pub fn kolaz_putanja(&self, id: &str) -> Option<PathBuf> {
        let sigurno: String = id
            .chars()
            .map(|z| if z.is_ascii_alphanumeric() || z == '-' || z == '_' { z } else { '_' })
            .collect();
        Some(self.art_dir.as_ref()?.join(format!("kolaz-{sigurno}.jpg")))
    }
}

/// Imena datoteka koje Rustiio prihvaca kao sliku mape.
pub const IMENA_MAPNE_SLIKE: [&str; 7] =
    ["folder.jpg", "folder.jpeg", "folder.png", "poster.jpg", "cover.jpg", "thumb.jpg", "default.jpg"];

/// Sastavi kolaz (2x2, 2x1 ili 1x1) od postera i spremi ga kao JPEG.
pub fn napravi_kolaz(posteri: &[PathBuf], izlaz: &Path, velicina: u32) -> Option<()> {
    use image::imageops::FilterType;
    let koliko = posteri.len().min(4);
    if koliko == 0 {
        return None;
    }
    let (kolone, redova) = match koliko {
        1 => (1u32, 1u32),
        2 => (2, 1),
        _ => (2, 2),
    };
    let sirina = velicina / kolone;
    let visina = velicina / redova;
    let mut platno = image::RgbImage::new(sirina * kolone, visina * redova);
    // Ako postera ima manje od mjesta u mrezi, ponavljamo ih da ne ostane crna polja.
    let mjesta = (kolone * redova) as usize;
    for (i, putanja) in (0..mjesta).map(|i| (i, &posteri[i % koliko])) {
        let Ok(slika) = image::open(putanja) else {
            continue;
        };
        let slika = slika.resize_to_fill(sirina, visina, FilterType::Lanczos3).to_rgb8();
        let x = (i as u32 % kolone) as i64 * sirina as i64;
        let y = (i as u32 / kolone) as i64 * visina as i64;
        image::imageops::overlay(&mut platno, &slika, x, y);
    }
    if let Some(roditelj) = izlaz.parent() {
        let _ = std::fs::create_dir_all(roditelj);
    }
    platno.save(izlaz).ok()
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
