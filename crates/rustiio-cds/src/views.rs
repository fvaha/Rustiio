//! Virtualne kategorije na vrhu stabla: Filmovi, Serije, Video, Nedavno dodano.
//!
//! Koje se nude odreduje `library.view_list` (Postavke → Knjižnica): korisnik
//! upiše `movies`, `series`, `video`, `recent`… pa TV vidi samo to.
//!
//! Ovo je odgovor na "gdje mi je film ako nije u mapi koju sam pogodio" — TV dobije
//! ravan popis po vrsti sadrzaja neovisno o rasporedu mapa na disku. ID-evi imaju
//! prefiks `v:` da se nikad ne sudare s numerickim ID-jevima kataloga.

use rustiio_library::{Catalog, Node, NodeKind};

use rustiio_upnp::didl;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Filmovi — video zapisi koji ne pripadaju nijednoj seriji.
    Movies,
    /// Serije — serijali posloženi po sezonama (`s:slug`).
    Series,
    Video,
    Audio,
    Image,
    Recent,
}

/// Sve kategorije koje se mogu izabrati (redoslijed u Postavkama).
pub const AVAILABLE: [View; 6] = [
    View::Movies,
    View::Series,
    View::Video,
    View::Recent,
    View::Audio,
    View::Image,
];

/// Zadano kad `library.view_list` ne kaže drugačije: filmovi i serije.
pub const DEFAULT_LIST: [View; 2] = [View::Movies, View::Series];

const VIDEO_KINDS: [NodeKind; 1] = [NodeKind::Video];
const AUDIO_KINDS: [NodeKind; 1] = [NodeKind::Audio];
const IMAGE_KINDS: [NodeKind; 1] = [NodeKind::Image];
const MEDIA_KINDS: [NodeKind; 3] = [NodeKind::Video, NodeKind::Audio, NodeKind::Image];

impl View {
    /// UPnP ObjectID virtualne kategorije.
    pub fn id(self) -> &'static str {
        match self {
            View::Movies => "v:movies",
            View::Series => "v:series",
            View::Video => "v:video",
            View::Audio => "v:audio",
            View::Image => "v:image",
            View::Recent => "v:recent",
        }
    }

    /// Ime za config (`library.view_list`) — kratko, bez prefiksa.
    pub fn name(self) -> &'static str {
        match self {
            View::Movies => "movies",
            View::Series => "series",
            View::Video => "video",
            View::Audio => "audio",
            View::Image => "image",
            View::Recent => "recent",
        }
    }

    /// Naslov koji TV prikazuje, na jeziku sučelja (`hr`/`en`).
    pub fn title(self, language: &str) -> &'static str {
        let en = language.eq_ignore_ascii_case("en");
        match self {
            View::Movies => {
                if en {
                    "Movies"
                } else {
                    "Filmovi"
                }
            }
            View::Series => {
                if en {
                    "Series"
                } else {
                    "Serije"
                }
            }
            View::Video => "Video",
            View::Audio => {
                if en {
                    "Music"
                } else {
                    "Muzika"
                }
            }
            View::Image => {
                if en {
                    "Photos"
                } else {
                    "Slike"
                }
            }
            View::Recent => {
                if en {
                    "Recently added"
                } else {
                    "Nedavno dodano"
                }
            }
        }
    }

    fn kinds(self) -> &'static [NodeKind] {
        match self {
            View::Movies | View::Series => &VIDEO_KINDS,
            View::Video => &VIDEO_KINDS,
            View::Audio => &AUDIO_KINDS,
            View::Image => &IMAGE_KINDS,
            View::Recent => &MEDIA_KINDS,
        }
    }

    /// Objekti u kategoriji (za "Nedavno dodano" samo zadnjih `recent_limit`).
    pub fn items(self, catalog: &Catalog, recent_limit: u32) -> Vec<Node> {
        match self {
            View::Recent => catalog.recent(self.kinds(), recent_limit.max(1) as usize),
            View::Movies => filmovi(catalog),
            View::Series => serije(catalog),
            _ => catalog.of_kinds(self.kinds()),
        }
    }

    pub fn count(self, catalog: &Catalog, recent_limit: u32) -> u32 {
        match self {
            View::Recent => (catalog.count_of_kinds(self.kinds()) as u32).min(recent_limit.max(1)),
            View::Movies => filmovi(catalog).len() as u32,
            View::Series => serije(catalog).len() as u32,
            _ => catalog.count_of_kinds(self.kinds()) as u32,
        }
    }

    /// Pozicija u popisu kategorija (za sortiranje na vrhu stabla).
    pub fn position(self) -> usize {
        AVAILABLE
            .iter()
            .position(|view| *view == self)
            .unwrap_or(usize::MAX)
    }

    /// Sinteticni cvor (nije na disku) — koristi se za DIDL i za sortiranje.
    pub fn node(self, language: &str) -> Node {
        Node {
            id: self.id().to_string(),
            parent_id: "0".to_string(),
            title: self.title(language).to_string(),
            kind: NodeKind::Container,
            path: std::path::PathBuf::new(),
            size: 0,
            modified: None,
            children: Vec::new(),
            subtitle: None,
        }
    }

    /// UPnP klasa za objekt kategorije (uvijek storageFolder).
    pub fn class(self) -> &'static str {
        didl::CLASS_STORAGE_FOLDER
    }
}

/// Je li ID virtualna kategorija.
pub fn is_view_id(id: &str) -> bool {
    id.starts_with("v:")
}

/// Nadji kategoriju po ID-u.
pub fn find(id: &str) -> Option<View> {
    AVAILABLE.into_iter().find(|view| view.id() == id)
}

/// Kategorije iz configa — prihvaća imena (`movies`) i ID-eve (`v:movies`),
/// nepoznato preskaće, a prazan popis znaci „zadano“ (filmovi i serije).
pub fn list(names: &[String]) -> Vec<View> {
    let mut izabrane: Vec<View> = Vec::new();
    for name in names {
        let kljuc = name.trim().trim_start_matches("v:").to_ascii_lowercase();
        if let Some(view) = AVAILABLE
            .iter()
            .find(|view| view.name() == kljuc || view.id() == name.trim())
            && !izabrane.contains(view)
        {
            izabrane.push(*view);
        }
    }
    if izabrane.is_empty() {
        DEFAULT_LIST.to_vec()
    } else {
        izabrane
    }
}

/// Je li cvor (ili neki predak) izmisljena serija/sezona (`s:…`).
fn unutar_serije(catalog: &Catalog, node: &Node) -> bool {
    let mut id = node.parent_id.clone();
    let mut koraka = 0;
    while id != "0" && !id.is_empty() && koraka < 64 {
        if id.starts_with("s:") {
            return true;
        }
        match catalog.get(&id) {
            Some(roditelj) => id = roditelj.parent_id.clone(),
            None => break,
        }
        koraka += 1;
    }
    false
}

/// Cvor serije (`s:slug`, bez sezone u ID-u).
fn je_serija(node: &Node) -> bool {
    node.is_container() && node.id.starts_with("s:") && !node.id["s:".len()..].contains(':')
}

/// Serijali iz cijelog kataloga.
fn serije(catalog: &Catalog) -> Vec<Node> {
    catalog.nodes().filter(|node| je_serija(node)).cloned().collect()
}

/// Video zapisi koji ne pripadaju ni jednoj seriji.
fn filmovi(catalog: &Catalog) -> Vec<Node> {
    catalog
        .nodes()
        .filter(|node| node.kind == NodeKind::Video && !unutar_serije(catalog, node))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustiio_core::config::{Root, RootKind};
    use rustiio_library::{ScanOptions, scan};

    fn catalog(tag: &str, files: &[(&str, usize)]) -> (std::path::PathBuf, Catalog) {
        let dir = std::env::temp_dir().join(format!("rustiio-views-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("podmapa")).unwrap();
        for (name, bytes) in files {
            let path = dir.join(name);
            std::fs::write(&path, vec![0u8; *bytes]).unwrap();
        }
        let catalog = scan(&ScanOptions::new(
            vec![Root { label: "M".to_string(), path: dir.clone(), kind: RootKind::Mixed }],
            rustiio_core::config::default_video_extensions(),
        ));
        (dir, catalog)
    }

    #[test]
    fn views_have_stable_ids_and_titles() {
        assert_eq!(View::Video.id(), "v:video");
        assert_eq!(View::Movies.id(), "v:movies");
        assert_eq!(View::Series.id(), "v:series");
        assert_eq!(View::Recent.title("hr"), "Nedavno dodano");
        assert_eq!(View::Recent.title("en"), "Recently added");
        assert_eq!(View::Movies.title("en"), "Movies");
        assert_eq!(View::Movies.title("hr"), "Filmovi");
        assert_eq!(find("7"), None);
        assert!(is_view_id("v:series"));
        assert!(!is_view_id("12"));
    }

    #[test]
    fn config_list_accepts_names_and_ids_and_has_a_default() {
        let izbor = list(&["series".into(), "v:movies".into(), "nepoznato".into()]);
        assert_eq!(izbor, vec![View::Series, View::Movies]);
        assert_eq!(list(&[]), vec![View::Movies, View::Series], "prazno → zadano");
        assert_eq!(list(&["video".into()]), vec![View::Video]);
        // Bez duplikata ako je isto upisano dva puta.
        assert_eq!(list(&["movies".into(), "v:movies".into()]), vec![View::Movies]);
    }

    #[test]
    fn movies_and_series_split_episodes_from_films() {
        let (dir, mut catalog) = catalog(
            "podjela",
            &[("Film.mkv", 10), ("podmapa/S01E01.mkv", 10), ("podmapa/S01E02.mkv", 10)],
        );
        let korijen = catalog.top_level().first().map(|node| node.id.clone()).expect("korijen");
        rustiio_library::grouping::arrange_video(&mut catalog, &korijen, false);
        let filmovi = View::Movies.items(&catalog, 20);
        assert_eq!(filmovi.len(), 1, "samo film: {:?}", filmovi.iter().map(|n| &n.title).collect::<Vec<_>>());
        assert_eq!(filmovi[0].title, "Film");
        let serije = View::Series.items(&catalog, 20);
        assert_eq!(serije.len(), 1);
        assert!(serije[0].id.starts_with("s:"), "serija je sinteticki cvor: {}", serije[0].id);
        assert_eq!(View::Series.count(&catalog, 20), 1);
        assert_eq!(View::Movies.count(&catalog, 20), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn video_view_collects_files_from_subfolders() {
        let (dir, catalog) = catalog("video", &[("a.mkv", 10), ("podmapa/b.mp4", 10), ("c.mp3", 10)]);
        let items = View::Video.items(&catalog, 20);
        let names: Vec<&str> = items.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(items.len(), 2, "oba videa, iz svih podmapa: {names:?}");
        // Naslovi su očišćeni za prikaz (malo ime se velikim slovom piše).
        assert!(names.contains(&"A"));
        assert!(names.contains(&"B"));
        assert_eq!(View::Video.count(&catalog, 20), 2);
        assert_eq!(View::Audio.count(&catalog, 20), 1);
        assert_eq!(View::Image.count(&catalog, 20), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_view_is_capped_by_limit() {
        let (dir, catalog) = catalog("recent", &[("a.mkv", 10), ("b.mkv", 10), ("c.mkv", 10)]);
        assert_eq!(View::Recent.items(&catalog, 2).len(), 2);
        assert_eq!(View::Recent.count(&catalog, 2), 2);
        assert_eq!(View::Recent.items(&catalog, 0).len(), 1, "limit 0 se podize na 1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn synthesized_node_is_a_container_under_root() {
        let node = View::Video.node("hr");
        assert_eq!(node.id, "v:video");
        assert_eq!(node.parent_id, "0");
        assert!(node.is_container());
        assert_eq!(node.title, "Video");
    }
}
