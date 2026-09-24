//! Skeniranje mapa: rekurzivno citanje, klasifikacija po ekstenziji i
//! povezivanje titlova s filmom (`Film.mkv` + `Film.srt`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rustiio_core::config::Root;

use crate::{IMAGE_EXTENSIONS, SUBTITLE_EXTENSIONS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Container,
    Video,
    Audio,
    Image,
    Subtitle,
    Other,
}

#[derive(Debug, Clone)]
pub struct Node {
    /// ID u UPnP smislu (`0` = root, `1`, `2`, ...).
    pub id: String,
    pub parent_id: String,
    /// Ime bez ekstenzije za video (tako ga TV prikazuje ljepse), inace puno ime.
    pub title: String,
    pub kind: NodeKind,
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub children: Vec<String>,
    /// `.srt`/`.vtt` uz ovaj video, ako postoji.
    pub subtitle: Option<PathBuf>,
}

impl Node {
    pub fn is_container(&self) -> bool {
        self.kind == NodeKind::Container
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    }
}

/// Katalog objekata u memoriji.
#[derive(Debug, Default)]
pub struct Catalog {
    map: HashMap<String, Node>,
    /// UPnP `SystemUpdateID` — raste sa svakim skeniranjem.
    pub update_id: u32,
}

impl Catalog {
    pub fn insert(&mut self, node: Node) {
        self.map.insert(node.id.clone(), node);
    }

    pub fn get(&self, id: &str) -> Option<&Node> {
        self.map.get(id)
    }

    /// Djeca kao klonovi (mali broj po mapi; veliki direktoriji se paginiraju gore).
    pub fn children(&self, id: &str) -> Vec<Node> {
        self.map
            .get(id)
            .map(|node| node.children.iter().filter_map(|child| self.map.get(child)).cloned().collect())
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Statistika po vrsti — koristi je `/api/status` i `doctor`.
    pub fn counts(&self) -> HashMap<&'static str, usize> {
        let mut out = HashMap::new();
        for node in self.map.values() {
            let key = match node.kind {
                NodeKind::Container => "folders",
                NodeKind::Video => "videos",
                NodeKind::Audio => "audio",
                NodeKind::Image => "images",
                NodeKind::Subtitle => "subtitles",
                NodeKind::Other => "other",
            };
            *out.entry(key).or_insert(0) += 1;
        }
        out
    }

    /// Svi objekti ispod roota (bez samog roota) — osnova za virtualne kategorije.
    pub fn flatten(&self) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        let mut stack: Vec<String> = vec!["0".to_string()];
        while let Some(id) = stack.pop() {
            let Some(node) = self.map.get(&id) else { continue };
            for child in &node.children {
                if let Some(child_node) = self.map.get(child) {
                    out.push(child_node.clone());
                    if child_node.is_container() {
                        stack.push(child.clone());
                    }
                }
            }
        }
        out
    }

    /// Svi objekti zadanih vrsta (npr. samo video), u dubinu.
    pub fn of_kinds(&self, kinds: &[NodeKind]) -> Vec<Node> {
        self.flatten().into_iter().filter(|node| kinds.contains(&node.kind)).collect()
    }

    /// Broj objekata zadanih vrsta — bez kloniranja (za `childCount` kategorija).
    pub fn count_of_kinds(&self, kinds: &[NodeKind]) -> usize {
        self.map.values().filter(|node| kinds.contains(&node.kind)).count()
    }

    /// Zadnjih `limit` objekata zadanih vrsta po vremenu izmjene ("Nedavno dodano").
    pub fn recent(&self, kinds: &[NodeKind], limit: usize) -> Vec<Node> {
        let mut items = self.of_kinds(kinds);
        items.sort_by(|a, b| b.modified.cmp(&a.modified));
        items.truncate(limit);
        items
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    pub roots: Vec<Root>,
    pub extensions: Vec<String>,
    pub max_depth: u32,
}

impl ScanOptions {
    pub fn new(roots: Vec<Root>, extensions: Vec<String>) -> Self {
        Self { roots, extensions, max_depth: 8 }
    }
}

/// Skeniraj sve root mape i vrati gotov katalog (root objekt `0`).
pub fn scan(options: &ScanOptions) -> Catalog {
    let mut catalog = Catalog::default();
    let mut next_id: u64 = 1;
    let mut root_children = Vec::new();

    for (index, root) in options.roots.iter().enumerate() {
        if !root.path.is_dir() {
            continue;
        }
        let id = next_id.to_string();
        next_id += 1;
        let title = if root.label.is_empty() {
            root.path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("Mapa {}", index + 1))
        } else {
            root.label.clone()
        };
        let children = scan_dir(&mut catalog, &mut next_id, &id, &root.path, options, 0);
        catalog.insert(Node {
            id: id.clone(),
            parent_id: "0".to_string(),
            title,
            kind: NodeKind::Container,
            path: root.path.clone(),
            size: 0,
            modified: None,
            children,
            subtitle: None,
        });
        root_children.push(id);
    }

    catalog.insert(Node {
        id: "0".to_string(),
        parent_id: "-1".to_string(),
        title: rustiio_core::APP_NAME.to_string(),
        kind: NodeKind::Container,
        path: PathBuf::new(),
        size: 0,
        modified: None,
        children: root_children,
        subtitle: None,
    });
    catalog.update_id = catalog.update_id.wrapping_add(1);
    catalog
}

fn scan_dir(
    catalog: &mut Catalog,
    next_id: &mut u64,
    parent_id: &str,
    dir: &Path,
    options: &ScanOptions,
    depth: u32,
) -> Vec<String> {
    if depth >= options.max_depth {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };

    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with('$') {
            continue; // skriveno i smece (Thumbs.db, .DS_Store)
        }
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            dirs.push(entry.path());
        } else if kind.is_file() {
            files.push(entry.path());
        }
        // Symlinkovi se namjerno ne slijede (petlje).
    }
    dirs.sort();
    files.sort();

    let mut ids = Vec::new();
    for sub in dirs {
        let id = next_id.to_string();
        *next_id += 1;
        let children = scan_dir(catalog, next_id, &id, &sub, options, depth + 1);
        catalog.insert(Node {
            id: id.clone(),
            parent_id: parent_id.to_string(),
            title: file_name_of(&sub),
            kind: NodeKind::Container,
            path: sub,
            size: 0,
            modified: None,
            children,
            subtitle: None,
        });
        ids.push(id);
    }

    // Titlovi se prvo popisu, pa se lijepе na video s istim imenom.
    let mut subtitles: HashMap<String, PathBuf> = HashMap::new();
    for file in &files {
        if is_subtitle(file) {
            subtitles.insert(stem_lower(file), file.clone());
        }
    }

    for file in files {
        let ext = extension_lower(&file);
        let kind = classify(&ext);
        let accepted = match kind {
            NodeKind::Video => options.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)),
            NodeKind::Audio => matches!(ext.as_str(), "mp3" | "m4a" | "aac" | "flac" | "wav" | "ogg"),
            _ => false,
        };
        if !accepted {
            continue;
        }
        let id = next_id.to_string();
        *next_id += 1;
        let meta = fs::metadata(&file).ok();
        let title = if kind == NodeKind::Video { stem_of(&file) } else { file_name_of(&file) };
        catalog.insert(Node {
            id: id.clone(),
            parent_id: parent_id.to_string(),
            title,
            kind,
            path: file.clone(),
            size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: meta.as_ref().and_then(|m| m.modified().ok()),
            children: Vec::new(),
            subtitle: subtitles.get(&stem_lower(&file)).cloned(),
        });
        ids.push(id);
    }

    ids
}

/// Klasifikacija po ekstenziji.
pub fn classify(ext: &str) -> NodeKind {
    let ext = ext.to_ascii_lowercase();
    if SUBTITLE_EXTENSIONS.contains(&ext.as_str()) {
        NodeKind::Subtitle
    } else if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        NodeKind::Image
    } else if matches!(
        ext.as_str(),
        "mkv"
            | "mp4"
            | "m4v"
            | "avi"
            | "mov"
            | "ts"
            | "m2ts"
            | "mts"
            | "webm"
            | "mpg"
            | "mpeg"
            | "wmv"
            | "flv"
            | "divx"
            | "vob"
            | "ogv"
            | "3gp"
    ) {
        NodeKind::Video
    } else if matches!(ext.as_str(), "mp3" | "m4a" | "aac" | "flac" | "wav" | "ogg" | "oga") {
        NodeKind::Audio
    } else {
        NodeKind::Other
    }
}

fn is_subtitle(path: &Path) -> bool {
    let ext = extension_lower(path);
    SUBTITLE_EXTENSIONS.contains(&ext.as_str())
}

fn extension_lower(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn stem_of(path: &Path) -> String {
    path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

fn stem_lower(path: &Path) -> String {
    stem_of(path).to_ascii_lowercase()
}

fn file_name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustiio_core::config::RootKind;

    struct TempTree {
        dir: PathBuf,
    }

    impl TempTree {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("rustiio-scan-{}", uuid_like()));
            fs::create_dir_all(&dir).unwrap();
            Self { dir }
        }

        fn file(&self, rel: &str, bytes: usize) -> PathBuf {
            let path = self.dir.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, vec![0u8; bytes]).unwrap();
            path
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    /// Jedinstveno ime temp mape: sat vremena nije dovoljan (dva testa u istoj
    /// nanosekundi dijele mapu), zato ide i atomicni brojac.
    fn uuid_like() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{nanos}-{:?}-{seq}", std::process::id())
    }

    fn options(dir: &Path) -> ScanOptions {
        ScanOptions::new(
            vec![rustiio_core::config::Root {
                label: "Filmovi".to_string(),
                path: dir.to_path_buf(),
                kind: RootKind::Video,
            }],
            rustiio_core::config::default_video_extensions(),
        )
    }

    #[test]
    fn scans_tree_and_classifies_files() {
        let tree = TempTree::new();
        tree.file("Film.mkv", 1024);
        tree.file("Serije/S01/Epizoda.mp4", 512);
        tree.file("notes.txt", 10);
        tree.file(".hidden.mkv", 10);

        let catalog = scan(&options(&tree.dir));
        let counts = catalog.counts();
        assert_eq!(counts.get("videos"), Some(&2));
        assert_eq!(counts.get("folders"), Some(&4), "root + Filmovi + Serije + S01");
        assert_eq!(counts.get("other"), None, "txt se ne indeksira");

        let root = catalog.get("0").expect("root");
        assert_eq!(root.children.len(), 1);
        let root_child = catalog.get(&root.children[0]).expect("root child");
        assert_eq!(root_child.title, "Filmovi");
    }

    #[test]
    fn title_is_stem_without_extension_and_size_is_recorded() {
        let tree = TempTree::new();
        tree.file("The Movie (2019).mkv", 2048);
        let catalog = scan(&options(&tree.dir));
        let root_child = catalog.get("1").expect("root child");
        let movies: Vec<Node> = catalog.children(&root_child.id);
        assert_eq!(movies.len(), 1);
        assert_eq!(movies[0].title, "The Movie (2019)");
        assert_eq!(movies[0].size, 2048);
        assert!(movies[0].modified.is_some());
    }

    #[test]
    fn subtitle_is_attached_to_video() {
        let tree = TempTree::new();
        tree.file("Film.mkv", 100);
        tree.file("Film.srt", 20);
        tree.file("Drugi.srt", 20);

        let catalog = scan(&options(&tree.dir));
        let movies = catalog.children("1");
        assert_eq!(movies.len(), 1, "titl se ne prikazuje kao zaseban objekt");
        let subtitle = movies[0].subtitle.as_ref().expect("titl spojen");
        assert!(subtitle.to_string_lossy().ends_with("Film.srt"));
    }

    #[test]
    fn classify_matches_extensions() {
        assert_eq!(classify("MKV"), NodeKind::Video);
        assert_eq!(classify("mp3"), NodeKind::Audio);
        assert_eq!(classify("srt"), NodeKind::Subtitle);
        assert_eq!(classify("png"), NodeKind::Image);
        assert_eq!(classify("pdf"), NodeKind::Other);
    }

    #[test]
    fn missing_root_is_skipped_without_panic() {
        let options = ScanOptions::new(
            vec![rustiio_core::config::Root {
                label: "Nema".to_string(),
                path: PathBuf::from("/nema/ovakve/mape/xyz"),
                kind: RootKind::Video,
            }],
            rustiio_core::config::default_video_extensions(),
        );
        let catalog = scan(&options);
        assert_eq!(catalog.get("0").unwrap().children.len(), 0);
        assert_eq!(catalog.len(), 1, "samo root objekt");
    }
}
