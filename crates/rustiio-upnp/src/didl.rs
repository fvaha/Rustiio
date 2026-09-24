//! DIDL-Lite — XML u kojem TV dobiva popis sadrzaja.
//!
//! Ovo je najosjetljiviji dio DLNA: imena namespacea, redoslijed elemenata i
//! atributi na `<res>` odlucuju hoce li Samsung/Sharp uopce prikazati film.
//! Zato ga gradimo rucno i drzimo oblik fiksnim (Serviio-kompatibilan).

use crate::escape;

/// Zaglavlje s namespaceima; `sec` je Samsungov dodatak za titlove.
pub const DIDL_HEADER: &str = concat!(
    r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/""#,
    r#" xmlns:dc="http://purl.org/dc/elements/1.1/""#,
    r#" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/""#,
    r#" xmlns:dlna="urn:schemas-dlna-org:metadata-1-0/""#,
    r#" xmlns:sec="http://www.sec.co.kr/">"#
);

/// UPnP klase koje koristimo.
pub const CLASS_STORAGE_FOLDER: &str = "object.container.storageFolder";
pub const CLASS_VIDEO_ITEM: &str = "object.item.videoItem";
pub const CLASS_AUDIO_ITEM: &str = "object.item.audioItem.musicTrack";
pub const CLASS_IMAGE_ITEM: &str = "object.item.imageItem.photo";

#[derive(Debug, Clone)]
pub struct Object {
    pub id: String,
    pub parent_id: String,
    pub title: String,
    pub class: String,
    pub is_container: bool,
    pub restricted: bool,
    pub date: Option<String>,
    pub album_art: Option<String>,
    pub child_count: Option<u32>,
    pub resources: Vec<Resource>,
}

impl Object {
    pub fn container(id: &str, parent_id: &str, title: &str, child_count: u32) -> Self {
        Self {
            id: id.to_string(),
            parent_id: parent_id.to_string(),
            title: title.to_string(),
            class: CLASS_STORAGE_FOLDER.to_string(),
            is_container: true,
            restricted: true,
            date: None,
            album_art: None,
            child_count: Some(child_count),
            resources: Vec::new(),
        }
    }

    pub fn item(id: &str, parent_id: &str, title: &str, class: &str) -> Self {
        Self {
            id: id.to_string(),
            parent_id: parent_id.to_string(),
            title: title.to_string(),
            class: class.to_string(),
            is_container: false,
            restricted: true,
            date: None,
            album_art: None,
            child_count: None,
            resources: Vec::new(),
        }
    }

    pub fn with_resource(mut self, resource: Resource) -> Self {
        self.resources.push(resource);
        self
    }

    pub fn with_date(mut self, date: String) -> Self {
        self.date = Some(date);
        self
    }

    /// Poster objekta (`upnp:albumArtURI` s `dlna:profileID="JPEG_TN"`).
    ///
    /// Zovemo ga samo kad URL stvarno postoji — prazan `albumArtURI` neki TV-i
    /// pokušavaju dohvatiti u nedogled.
    pub fn with_album_art(mut self, url: &str) -> Self {
        self.album_art = Some(url.to_string());
        self
    }
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub url: String,
    pub protocol_info: String,
    pub size: Option<u64>,
    pub duration: Option<String>,
    pub resolution: Option<String>,
    pub bitrate: Option<u32>,
    /// `sec:CaptionInfoEx` — Samsung/Sharp tako nalaze titl uz video.
    pub caption_url: Option<String>,
    pub caption_type: Option<String>,
}

impl Resource {
    pub fn new(url: &str, protocol_info: &str) -> Self {
        Self {
            url: url.to_string(),
            protocol_info: protocol_info.to_string(),
            size: None,
            duration: None,
            resolution: None,
            bitrate: None,
            caption_url: None,
            caption_type: None,
        }
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_caption(mut self, url: &str, kind: &str) -> Self {
        self.caption_url = Some(url.to_string());
        self.caption_type = Some(kind.to_string());
        self
    }
}

/// Renderiraj popis objekata u jedan `<DIDL-Lite>` dokument.
pub fn render(objects: &[Object]) -> String {
    let mut out = String::with_capacity(DIDL_HEADER.len() + 256 + objects.len() * 480);
    out.push_str(DIDL_HEADER);
    for object in objects {
        if object.is_container {
            render_container(&mut out, object);
        } else {
            render_item(&mut out, object);
        }
    }
    out.push_str("</DIDL-Lite>");
    out
}

fn render_container(out: &mut String, object: &Object) {
    out.push_str(&format!(
        r#"<container id="{id}" parentID="{parent}" restricted="{restricted}" childCount="{count}">"#,
        id = escape(&object.id),
        parent = escape(&object.parent_id),
        restricted = if object.restricted { 1 } else { 0 },
        count = object.child_count.unwrap_or(0),
    ));
    render_common(out, object);
    out.push_str("</container>");
}

fn render_item(out: &mut String, object: &Object) {
    out.push_str(&format!(
        r#"<item id="{id}" parentID="{parent}" restricted="{restricted}">"#,
        id = escape(&object.id),
        parent = escape(&object.parent_id),
        restricted = if object.restricted { 1 } else { 0 },
    ));
    render_common(out, object);
    out.push_str("</item>");
}

fn render_common(out: &mut String, object: &Object) {
    out.push_str(&format!("<dc:title>{}</dc:title>", escape(&object.title)));
    out.push_str(&format!("<upnp:class>{}</upnp:class>", escape(&object.class)));
    if let Some(date) = &object.date {
        out.push_str(&format!("<dc:date>{}</dc:date>", escape(date)));
    }
    if let Some(art) = &object.album_art {
        out.push_str(&format!(
            r#"<upnp:albumArtURI dlna:profileID="JPEG_TN">{}</upnp:albumArtURI>"#,
            escape(art)
        ));
    }
    for resource in &object.resources {
        render_resource(out, resource);
    }
}

fn render_resource(out: &mut String, resource: &Resource) {
    out.push_str(&format!(r#"<res protocolInfo="{}""#, escape(&resource.protocol_info)));
    if let Some(size) = resource.size {
        out.push_str(&format!(r#" size="{size}""#));
    }
    if let Some(duration) = &resource.duration {
        out.push_str(&format!(r#" duration="{}""#, escape(duration)));
    }
    if let Some(resolution) = &resource.resolution {
        out.push_str(&format!(r#" resolution="{}""#, escape(resolution)));
    }
    if let Some(bitrate) = resource.bitrate {
        out.push_str(&format!(r#" bitrate="{bitrate}""#));
    }
    if let (Some(url), Some(kind)) = (&resource.caption_url, &resource.caption_type) {
        out.push_str(&format!(r#" sec:CaptionInfoEx="{}" sec:type="{}""#, escape(url), escape(kind)));
    }
    out.push('>');
    out.push_str(&escape(&resource.url));
    out.push_str("</res>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_and_item_are_rendered_with_classes() {
        let folder = Object::container("1", "0", "Filmovi", 12);
        let movie = Object::item("2", "1", "Film (2019).mkv", CLASS_VIDEO_ITEM).with_resource(
            Resource::new("http://10.0.0.1:8200/res/2/Film.mkv", "http-get:*:video/x-matroska:*")
                .with_size(1_500_000_000)
                .with_caption("http://10.0.0.1:8200/res/3/Film.srt", "srt"),
        );
        let xml = render(&[folder, movie]);

        assert!(xml.starts_with("<DIDL-Lite "));
        assert!(xml.ends_with("</DIDL-Lite>"));
        assert!(xml.contains(r#"<container id="1" parentID="0" restricted="1" childCount="12">"#));
        assert!(xml.contains("<upnp:class>object.container.storageFolder</upnp:class>"));
        assert!(xml.contains("<dc:title>Film (2019).mkv</dc:title>"));
        assert!(xml.contains(r#"size="1500000000""#));
        assert!(xml.contains(r#"sec:CaptionInfoEx="http://10.0.0.1:8200/res/3/Film.srt" sec:type="srt""#));
    }

    #[test]
    fn titles_with_specials_are_escaped() {
        let item = Object::item("9", "1", "Riba & Co <2019>", CLASS_VIDEO_ITEM);
        let xml = render(&[item]);
        assert!(xml.contains("<dc:title>Riba &amp; Co &lt;2019&gt;</dc:title>"));
        assert!(!xml.contains("Riba & Co"));
    }

    #[test]
    fn empty_list_is_still_valid_document() {
        let xml = render(&[]);
        assert_eq!(xml, format!("{DIDL_HEADER}</DIDL-Lite>"));
    }
}
