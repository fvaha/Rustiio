//! `Browse` i `Search` nad katalogom, s pagingom, sortiranjem i DIDL izlazom.

use rustiio_core::time::format_rfc3339;
use rustiio_library::{Catalog, Node, NodeKind};
use rustiio_upnp::didl::{self, Object, Resource};
use rustiio_upnp::protocol;
use rustiio_upnp::{escape_path_segment, render_didl};

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
    base_url: &str,
    max_results: u32,
) -> Result<BrowseOutcome, CdsError> {
    let object_id = if request.object_id.is_empty() { "0" } else { request.object_id.as_str() };
    let node = catalog.get(object_id).ok_or(CdsError::NoSuchObject)?;

    let metadata_only = request.browse_flag.eq_ignore_ascii_case("BrowseMetadata");
    if request.browse_flag.is_empty() {
        return Err(CdsError::InvalidArgs("fali BrowseFlag".to_string()));
    }

    let update_id = catalog.update_id;

    if metadata_only {
        let object = node_to_object(node, catalog, base_url);
        return Ok(BrowseOutcome { didl: render_didl(&[object]), total: 1, returned: 1, update_id });
    }

    let mut children = catalog.children(object_id);
    sort_nodes(&mut children, &request.sort_criteria);

    let total = children.len() as u32;
    let limit = if max_results == 0 { MAX_RESULTS } else { max_results };
    let wanted = if request.requested_count == 0 { limit } else { request.requested_count.min(limit) };
    let start = request.starting_index as usize;
    let page: Vec<Object> = children
        .iter()
        .skip(start)
        .take(wanted as usize)
        .map(|child| node_to_object(child, catalog, base_url))
        .collect();

    Ok(BrowseOutcome { returned: page.len() as u32, didl: render_didl(&page), total, update_id })
}

/// Pretvori cvor kataloga u DIDL objekt s resursima (i titlom, ako ga ima).
pub fn node_to_object(node: &Node, catalog: &Catalog, base_url: &str) -> Object {
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

    let url = format!("{base_url}/res/{}/{file_name}", node.id);
    let mut resource = Resource::new(&url, &info.to_protocol_info()).with_size(node.size);

    if let Some(subtitle) = &node.subtitle {
        let sub_name = escape_path_segment(
            &subtitle.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        );
        let sub_ext =
            subtitle.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let sub_url = format!("{base_url}/sub/{}/{sub_name}", node.id);
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
        let sub_url = format!("{base_url}/sub/{}/{sub_name}", node.id);
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

fn sort_nodes(nodes: &mut [Node], criteria: &str) {
    let first = criteria.split(',').next().unwrap_or("").trim().to_string();
    if first.is_empty() {
        // TV nije rekao kako sortirati — mapе prvo, pa abecedno.
        nodes.sort_by(|a, b| {
            b.is_container()
                .cmp(&a.is_container())
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
        return;
    }
    let (descending, field) = match first.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, first.strip_prefix('+').unwrap_or(&first)),
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

    #[test]
    fn browse_root_returns_storage_folder() {
        let dir = temp_dir("root");
        std::fs::write(dir.join("A.mkv"), b"x").unwrap();
        let catalog = catalog_with(&dir);

        let request = BrowseRequest {
            object_id: "0".to_string(),
            browse_flag: "BrowseDirectChildren".to_string(),
            ..Default::default()
        };
        let out = browse(&catalog, &request, "http://10.0.0.1:8200", 0).expect("browse");
        assert_eq!(out.total, 1);
        assert_eq!(out.returned, 1);
        assert!(out.didl.contains("<dc:title>Filmovi</dc:title>"));
        assert!(out.didl.contains("object.container.storageFolder"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn browse_movie_has_playable_resource_and_subtitle() {
        let dir = temp_dir("movie");
        std::fs::write(dir.join("The Movie (2019).mkv"), vec![0u8; 64]).unwrap();
        std::fs::write(dir.join("The Movie (2019).srt"), b"1").unwrap();
        let catalog = catalog_with(&dir);

        let folder_id = catalog.get("0").unwrap().children[0].clone();
        let request = BrowseRequest {
            object_id: folder_id,
            browse_flag: "BrowseDirectChildren".to_string(),
            ..Default::default()
        };
        let out = browse(&catalog, &request, "http://10.0.0.1:8200", 0).expect("browse");

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
            object_id: "1".to_string(),
            browse_flag: "BrowseMetadata".to_string(),
            ..Default::default()
        };
        let out = browse(&catalog, &request, "http://10.0.0.1:8200", 0).expect("browse");
        assert_eq!(out.total, 1);
        assert!(out.didl.contains("<dc:title>Filmovi</dc:title>"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_object_is_701_and_bad_flag_is_402() {
        let dir = temp_dir("err");
        let catalog = catalog_with(&dir);

        let missing = BrowseRequest {
            object_id: "999".to_string(),
            browse_flag: "BrowseDirectChildren".to_string(),
            ..Default::default()
        };
        assert_eq!(browse(&catalog, &missing, "http://x", 0).map(|_| ()), Err(CdsError::NoSuchObject));
        assert_eq!(CdsError::NoSuchObject.code(), 701);

        let bad =
            BrowseRequest { object_id: "0".to_string(), browse_flag: String::new(), ..Default::default() };
        assert!(matches!(browse(&catalog, &bad, "http://x", 0), Err(CdsError::InvalidArgs(_))));
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
        let out = browse(&catalog, &request, "http://x", 0).expect("browse");
        assert_eq!(out.total, 4);
        assert_eq!(out.returned, 2);
        assert!(out.didl.contains("<dc:title>b</dc:title>"));
        assert!(out.didl.contains("<dc:title>c</dc:title>"));
        assert!(!out.didl.contains("<dc:title>a</dc:title>"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
