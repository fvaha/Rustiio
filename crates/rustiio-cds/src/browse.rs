//! `Browse` i `Search` nad katalogom, s pagingom, sortiranjem i DIDL izlazom.

use rustiio_core::time::format_rfc3339;
use rustiio_library::{Catalog, Node, NodeKind};
use rustiio_upnp::didl::{self, Object, Resource};
use rustiio_upnp::protocol;
use rustiio_upnp::{escape_path_segment, render_didl};

use crate::views::{self, View};

/// Najveci broj objekata u jednom odgovoru (TV-i traze 0 = "sve").
pub const MAX_RESULTS: u32 = 500;

#[derive(Debug, Clone, Default)]
pub struct BrowseRequest {
    pub object_id: String,
    pub browse_flag: String,
    pub filter: String,
    pub starting_index: u32,
    pub requested_count: u32,
    pub sort_criteria: String,
}

/// Kako server zeli posluziti objekt. `None` iz [`PlaybackResolver`]-a znaci
/// "originalni fajl" (direct play).
#[derive(Debug, Clone)]
pub struct Playback {
    /// Putanja bez `base_url`-a, npr. `/tr/5/film.mkv`.
    pub path: String,
    /// `protocolInfo` za taj resurs (bez PN-a kad je transcode).
    pub protocol_info: String,
}

/// Server odlucuje (profil uredjaja + transcode engine), CDS samo ispise.
///
/// Trait je namjerno ovako mali: `rustiio-cds` ne zna nista o profilima ni ffmpeg-u.
pub trait PlaybackResolver {
    fn resolve(&self, node: &Node) -> Option<Playback>;
}

/// Sve sto CDS treba znati o serveru (bez ovoga bi lista argumenata rasla svakom fazom).
///
/// `Debug`/`Default` su rucno napisani jer `playback` je trait objekt (nije Debug).
pub struct BrowseOptions<'a> {
    /// `http://192.168.1.10:8200`
    pub base_url: &'a str,
    /// Gornja granica objekata u odgovoru.
    pub max_results: u32,
    /// Prikazuj virtualne kategorije (Video/Muzika/Slike/Nedavno dodano).
    pub views: bool,
    /// Koliko objekata ide u "Nedavno dodano".
    pub recent_limit: u32,
    /// Ako uredjaj ne moze original, server daje drugu putanju (transcode).
    pub playback: Option<&'a dyn PlaybackResolver>,
}

impl std::fmt::Debug for BrowseOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowseOptions")
            .field("base_url", &self.base_url)
            .field("max_results", &self.max_results)
            .field("views", &self.views)
            .field("recent_limit", &self.recent_limit)
            .field("playback", &self.playback.is_some())
            .finish()
    }
}

impl Default for BrowseOptions<'_> {
    fn default() -> Self {
        Self { base_url: "", max_results: MAX_RESULTS, views: true, recent_limit: 20, playback: None }
    }
}

impl<'a> BrowseOptions<'a> {
    pub fn new(base_url: &'a str) -> Self {
        Self { base_url, ..Self::default() }
    }

    pub fn with_playback(mut self, playback: &'a dyn PlaybackResolver) -> Self {
        self.playback = Some(playback);
        self
    }
}

#[derive(Debug, Clone)]
pub struct BrowseOutcome {
    pub didl: String,
    pub total: u32,
    pub returned: u32,
    pub update_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CdsError {
    /// UPnP 701
    NoSuchObject,
    /// UPnP 402
    InvalidArgs(String),
    /// UPnP 501 — akcija postoji, ali je ne podrzavamo (jos).
    Unsupported(String),
}

impl CdsError {
    pub fn code(&self) -> u32 {
        match self {
            CdsError::NoSuchObject => 701,
            CdsError::InvalidArgs(_) => 402,
            CdsError::Unsupported(_) => 501,
        }
    }

    pub fn description(&self) -> String {
        match self {
            CdsError::NoSuchObject => "No such object".to_string(),
            CdsError::InvalidArgs(msg) => format!("Invalid args: {msg}"),
            CdsError::Unsupported(msg) => format!("Action not implemented: {msg}"),
        }
    }
}

/// Sta podrzavamo u `SortCriteria` (TV-i ovo pitaju prije nego sortiraju).
pub fn sort_capabilities() -> &'static str {
    "dc:title,dc:date,upnp:class"
}

/// `Browse` — `BrowseMetadata` vraca sam objekt, `BrowseDirectChildren` njegovu djecu.
pub fn browse(
    catalog: &Catalog,
    request: &BrowseRequest,
    options: &BrowseOptions<'_>,
) -> Result<BrowseOutcome, CdsError> {
    if request.browse_flag.is_empty() {
        return Err(CdsError::InvalidArgs("fali BrowseFlag".to_string()));
    }
    let update_id = catalog.update_id;
    let metadata_only = request.browse_flag.eq_ignore_ascii_case("BrowseMetadata");
    let object_id = if request.object_id.is_empty() { "0" } else { request.object_id.as_str() };

    // Virtualna kategorija (`v:video`, `v:recent`, ...).
    if let Some(view) = views::find(object_id) {
        if !options.views {
            return Err(CdsError::NoSuchObject);
        }
        if metadata_only {
            let object = view_object(view, catalog, options);
            return Ok(BrowseOutcome { didl: render_didl(&[object]), total: 1, returned: 1, update_id });
        }
        let mut items = view.items(catalog, options.recent_limit);
        sort_nodes(&mut items, &request.sort_criteria);
        return paginate(catalog, items, request, options, update_id);
    }

    let node = catalog.get(object_id).ok_or(CdsError::NoSuchObject)?;
    if metadata_only {
        let object = node_to_object(node, catalog, options);
        return Ok(BrowseOutcome { didl: render_didl(&[object]), total: 1, returned: 1, update_id });
    }

    let mut children = catalog.children(object_id);
    if object_id == "0" && options.views {
        for view in views::ALL {
            children.push(view.node());
        }
        order_root_children(&mut children, &request.sort_criteria);
    } else {
        sort_nodes(&mut children, &request.sort_criteria);
    }

    paginate(catalog, children, request, options, update_id)
}

fn paginate(
    catalog: &Catalog,
    children: Vec<Node>,
    request: &BrowseRequest,
    options: &BrowseOptions<'_>,
    update_id: u32,
) -> Result<BrowseOutcome, CdsError> {
    let total = children.len() as u32;
    let limit = if options.max_results == 0 { MAX_RESULTS } else { options.max_results };
    let wanted = if request.requested_count == 0 { limit } else { request.requested_count.min(limit) };
    let start = request.starting_index as usize;

    let objects: Vec<Object> = children
        .iter()
        .skip(start)
        .take(wanted as usize)
        .map(|child| match views::find(&child.id) {
            Some(view) => view_object(view, catalog, options),
            None => node_to_object(child, catalog, options),
        })
        .collect();

    Ok(BrowseOutcome { returned: objects.len() as u32, didl: render_didl(&objects), total, update_id })
}

fn view_object(view: View, catalog: &Catalog, options: &BrowseOptions<'_>) -> Object {
    Object::container(view.id(), "0", view.title(), view.count(catalog, options.recent_limit))
}

/// Pretvori cvor kataloga u DIDL objekt s resursima (i titlom, ako ga ima).
pub fn node_to_object(node: &Node, catalog: &Catalog, options: &BrowseOptions<'_>) -> Object {
    if node.is_container() {
        let child_count = catalog.children(&node.id).len() as u32;
        return Object::container(&node.id, &node.parent_id, &node.title, child_count);
    }

    let ext = node.path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let info = protocol::guess_for_ext(&ext);
    let file_name = escape_path_segment(&node.file_name());

    let class = match node.kind {
        NodeKind::Video => didl::CLASS_VIDEO_ITEM,
        NodeKind::Audio => didl::CLASS_AUDIO_ITEM,
        NodeKind::Image => didl::CLASS_IMAGE_ITEM,
        _ => didl::CLASS_VIDEO_ITEM,
    };

    // Ako uredjaj ne moze original, server vrati drugu putanju (remux/transcode) —
    // tada velicina i trajanje originala ne vrijede.
    let playback = options.playback.and_then(|resolver| resolver.resolve(node));
    let (url, protocol_info, size): (String, String, Option<u64>) = match &playback {
        Some(playback) => {
            (format!("{}{}", options.base_url, playback.path), playback.protocol_info.clone(), None)
        }
        None => (
            format!("{}/res/{}/{file_name}", options.base_url, node.id),
            info.to_protocol_info(),
            Some(node.size),
        ),
    };
    let mut resource = Resource::new(&url, &protocol_info);
    if let Some(size) = size {
        resource = resource.with_size(size);
    }

    if let Some(subtitle) = &node.subtitle {
        let sub_name = escape_path_segment(
            &subtitle.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        );
        let sub_ext =
            subtitle.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let sub_url = format!("{}/sub/{}/{sub_name}", options.base_url, node.id);
        resource = resource.with_caption(&sub_url, &sub_ext);
    }

    let mut object = Object::item(&node.id, &node.parent_id, &node.title, class).with_resource(resource);

    // Titl ide i kao zaseban resurs — neki klijenti (Kodi, VLC) citaju samo taj oblik.
    if let Some(subtitle) = &node.subtitle {
        let sub_ext =
            subtitle.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let sub_name = escape_path_segment(
            &subtitle.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        );
        let sub_url = format!("{}/sub/{}/{sub_name}", options.base_url, node.id);
        let sub_info = protocol::guess_for_ext(&sub_ext);
        object = object.with_resource(
            Resource::new(&sub_url, &sub_info.to_protocol_info())
                .with_size(std::fs::metadata(subtitle).map(|m| m.len()).unwrap_or(0)),
        );
    }

    if let Some(modified) = node.modified {
        object = object.with_date(format_rfc3339(modified));
    }

    object
}

/// Vrh stabla: kategorije prve (fiksni redoslijed), pa prave mape abecedno.
fn order_root_children(nodes: &mut Vec<Node>, criteria: &str) {
    if !criteria.trim().is_empty() {
        sort_nodes(nodes, criteria);
        return;
    }
    let mut categories = Vec::new();
    let mut rest = Vec::new();
    for node in nodes.drain(..) {
        if views::is_view_id(&node.id) {
            categories.push(node);
        } else {
            rest.push(node);
        }
    }
    categories.sort_by_key(|node| views::find(&node.id).map(|view| view.position()).unwrap_or(usize::MAX));
    rest.sort_by_key(|node| node.title.to_lowercase());
    nodes.extend(categories);
    nodes.extend(rest);
}

fn sort_nodes(nodes: &mut [Node], criteria: &str) {
    let first = criteria.split(',').next().unwrap_or("").trim();
    if first.is_empty() {
        // TV nije rekao kako sortirati — mape prvo, pa abecedno.
        nodes.sort_by(|a, b| {
            b.is_container()
                .cmp(&a.is_container())
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
        return;
    }
    let (descending, field) = match first.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, first.strip_prefix('+').unwrap_or(first)),
    };
    match field {
        "dc:date" => nodes.sort_by(|a, b| a.modified.cmp(&b.modified)),
        _ => nodes.sort_by_key(|n| n.title.to_lowercase()),
    }
    if descending {
        nodes.reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustiio_core::config::{Root, RootKind};

    fn catalog_with(dir: &std::path::Path) -> Catalog {
        rustiio_library::scan(&rustiio_library::ScanOptions::new(
            vec![Root { label: "Filmovi".to_string(), path: dir.to_path_buf(), kind: RootKind::Video }],
            rustiio_core::config::default_video_extensions(),
        ))
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rustiio-cds-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn options() -> BrowseOptions<'static> {
        BrowseOptions {
            base_url: "http://10.0.0.1:8200",
            max_results: MAX_RESULTS,
            views: true,
            recent_limit: 20,
            playback: None,
        }
    }

    fn children_request(object_id: &str) -> BrowseRequest {
        BrowseRequest {
            object_id: object_id.to_string(),
            browse_flag: "BrowseDirectChildren".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn browse_root_returns_storage_folder_and_categories() {
        let dir = temp_dir("root");
        std::fs::write(dir.join("A.mkv"), b"x").unwrap();
        let catalog = catalog_with(&dir);

        let out = browse(&catalog, &children_request("0"), &options()).expect("browse");
        assert_eq!(out.total, 5, "1 mapa + 4 kategorije");
        assert!(out.didl.contains("<dc:title>Filmovi</dc:title>"));
        assert!(out.didl.contains("object.container.storageFolder"));
        for title in ["Video", "Nedavno dodano", "Muzika", "Slike"] {
            assert!(out.didl.contains(&format!("<dc:title>{title}</dc:title>")), "fali {title}");
        }
        // kategorije idu prije pravih mapa
        assert!(out.didl.find("v:video").unwrap() < out.didl.find("<dc:title>Filmovi</dc:title>").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn views_can_be_disabled() {
        let dir = temp_dir("noviews");
        std::fs::write(dir.join("A.mkv"), b"x").unwrap();
        let catalog = catalog_with(&dir);

        let mut opts = options();
        opts.views = false;
        let out = browse(&catalog, &children_request("0"), &opts).expect("browse");
        assert_eq!(out.total, 1);
        assert!(!out.didl.contains("v:video"), "kategorija ne smije biti u listi");

        let direct = browse(&catalog, &children_request("v:video"), &opts);
        assert_eq!(direct.map(|_| ()), Err(CdsError::NoSuchObject));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn video_category_lists_movies_with_playable_urls() {
        let dir = temp_dir("category");
        std::fs::create_dir_all(dir.join("Serije")).unwrap();
        std::fs::write(dir.join("Serije/Epizoda.mkv"), vec![0u8; 32]).unwrap();
        let catalog = catalog_with(&dir);

        let out = browse(&catalog, &children_request("v:video"), &options()).expect("browse");
        assert_eq!(out.total, 1, "video iz podmape se vidi u kategoriji");
        assert!(out.didl.contains("<dc:title>Epizoda</dc:title>"));
        assert!(out.didl.contains("/res/"));
        assert!(out.didl.contains("video/x-matroska"));

        let meta = browse(
            &catalog,
            &BrowseRequest {
                object_id: "v:video".into(),
                browse_flag: "BrowseMetadata".into(),
                ..Default::default()
            },
            &options(),
        )
        .expect("metadata");
        assert!(meta.didl.contains(r#"childCount="1""#));
        assert!(meta.didl.contains("<dc:title>Video</dc:title>"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_category_is_limited() {
        let dir = temp_dir("recent");
        for name in ["a", "b", "c"] {
            std::fs::write(dir.join(format!("{name}.mkv")), b"x").unwrap();
        }
        let catalog = catalog_with(&dir);
        let mut opts = options();
        opts.recent_limit = 2;
        let out = browse(&catalog, &children_request("v:recent"), &opts).expect("browse");
        assert_eq!(out.total, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn browse_movie_has_playable_resource_and_subtitle() {
        let dir = temp_dir("movie");
        std::fs::write(dir.join("The Movie (2019).mkv"), vec![0u8; 64]).unwrap();
        std::fs::write(dir.join("The Movie (2019).srt"), b"1").unwrap();
        let catalog = catalog_with(&dir);

        let folder_id = catalog.get("0").unwrap().children[0].clone();
        let out = browse(&catalog, &children_request(&folder_id), &options()).expect("browse");

        assert!(out.didl.contains("http-get:*:video/x-matroska:DLNA.ORG_OP=01"));
        assert!(out.didl.contains("res/2/The%20Movie%20(2019).mkv"));
        assert!(out.didl.contains("sec:CaptionInfoEx"));
        assert!(out.didl.contains("text/srt"));
        assert!(out.didl.contains("<dc:date>"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn browse_metadata_returns_single_object() {
        let dir = temp_dir("meta");
        std::fs::write(dir.join("B.mp4"), b"x").unwrap();
        let catalog = catalog_with(&dir);

        let request = BrowseRequest {
            object_id: "1".into(),
            browse_flag: "BrowseMetadata".into(),
            ..Default::default()
        };
        let out = browse(&catalog, &request, &options()).expect("browse");
        assert_eq!(out.total, 1);
        assert!(out.didl.contains("<dc:title>Filmovi</dc:title>"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_object_is_701_and_bad_flag_is_402() {
        let dir = temp_dir("err");
        let catalog = catalog_with(&dir);
        let opts = options();

        let missing = children_request("999");
        assert_eq!(browse(&catalog, &missing, &opts).map(|_| ()), Err(CdsError::NoSuchObject));
        assert_eq!(CdsError::NoSuchObject.code(), 701);

        let bad = BrowseRequest { object_id: "0".into(), browse_flag: String::new(), ..Default::default() };
        assert!(matches!(browse(&catalog, &bad, &opts), Err(CdsError::InvalidArgs(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paging_respects_index_and_count() {
        let dir = temp_dir("paging");
        for name in ["a", "b", "c", "d"] {
            std::fs::write(dir.join(format!("{name}.mkv")), b"x").unwrap();
        }
        let catalog = catalog_with(&dir);
        let request = BrowseRequest {
            object_id: "1".to_string(),
            browse_flag: "BrowseDirectChildren".to_string(),
            starting_index: 1,
            requested_count: 2,
            sort_criteria: "+dc:title".to_string(),
            ..Default::default()
        };
        let out = browse(&catalog, &request, &options()).expect("browse");
        assert_eq!(out.total, 4);
        assert_eq!(out.returned, 2);
        assert!(out.didl.contains("<dc:title>b</dc:title>"));
        assert!(out.didl.contains("<dc:title>c</dc:title>"));
        assert!(!out.didl.contains("<dc:title>a</dc:title>"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
