//! Virtualne kategorije na vrhu stabla: Video, Muzika, Slike, Nedavno dodano.
//!
//! Ovo je odgovor na "gdje mi je film ako nije u mapi koju sam pogodio" — TV dobije
//! ravan popis po vrsti sadrzaja neovisno o rasporedu mapa na disku. ID-evi imaju
//! prefiks `v:` da se nikad ne sudare s numerickim ID-jevima kataloga.

use rustiio_library::{Catalog, Node, NodeKind};

use rustiio_upnp::didl;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Video,
    Audio,
    Image,
    Recent,
}

/// Redoslijed u kojem se kategorije pojavljuju na vrhu.
pub const ALL: [View; 4] = [View::Video, View::Recent, View::Audio, View::Image];

const VIDEO_KINDS: [NodeKind; 1] = [NodeKind::Video];
const AUDIO_KINDS: [NodeKind; 1] = [NodeKind::Audio];
const IMAGE_KINDS: [NodeKind; 1] = [NodeKind::Image];
const MEDIA_KINDS: [NodeKind; 3] = [NodeKind::Video, NodeKind::Audio, NodeKind::Image];

impl View {
    /// UPnP ObjectID virtualne kategorije.
    pub fn id(self) -> &'static str {
        match self {
            View::Video => "v:video",
            View::Audio => "v:audio",
            View::Image => "v:image",
            View::Recent => "v:recent",
        }
    }

    /// Naslov koji TV prikazuje.
    pub fn title(self) -> &'static str {
        match self {
            View::Video => "Video",
            View::Audio => "Muzika",
            View::Image => "Slike",
            View::Recent => "Nedavno dodano",
        }
    }

    fn kinds(self) -> &'static [NodeKind] {
        match self {
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
            _ => catalog.of_kinds(self.kinds()),
        }
    }

    pub fn count(self, catalog: &Catalog, recent_limit: u32) -> u32 {
        let total = catalog.count_of_kinds(self.kinds()) as u32;
        match self {
            View::Recent => total.min(recent_limit.max(1)),
            _ => total,
        }
    }

    /// Pozicija u popisu kategorija (za sortiranje na vrhu stabla).
    pub fn position(self) -> usize {
        ALL.iter().position(|view| *view == self).unwrap_or(usize::MAX)
    }

    /// Sinteticni cvor (nije na disku) — koristi se za DIDL i za sortiranje.
    pub fn node(self) -> Node {
        Node {
            id: self.id().to_string(),
            parent_id: "0".to_string(),
            title: self.title().to_string(),
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
    ALL.into_iter().find(|view| view.id() == id)
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
        assert_eq!(View::Recent.title(), "Nedavno dodano");
        assert_eq!(find("v:image"), Some(View::Image));
        assert_eq!(find("7"), None);
        assert!(is_view_id("v:recent"));
        assert!(!is_view_id("12"));
    }

    #[test]
    fn video_view_collects_files_from_subfolders() {
        let (dir, catalog) = catalog("video", &[("a.mkv", 10), ("podmapa/b.mp4", 10), ("c.mp3", 10)]);
        let items = View::Video.items(&catalog, 20);
        let names: Vec<&str> = items.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(items.len(), 2, "oba videa, iz svih podmapa: {names:?}");
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
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
        let node = View::Video.node();
        assert_eq!(node.id, "v:video");
        assert_eq!(node.parent_id, "0");
        assert!(node.is_container());
        assert_eq!(node.title, "Video");
    }
}
