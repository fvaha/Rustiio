//! `device.xml` — opis uredaja koji klijent prvo dohvati s `LOCATION` URL-a.
//!
//! TV-i (i VLC) iz ovoga uce: ime, tip uredaja (MediaServer:1) i popis servisa
//! s njihovim control/SCPD/event URL-ovima.

use crate::escape;
use rustiio_core::{DEVICE_TYPE, SERVICE_CONNECTION_MANAGER, SERVICE_CONTENT_DIRECTORY};

#[derive(Debug, Clone)]
pub struct DeviceMeta {
    /// `uuid:...`
    pub udn: String,
    pub friendly_name: String,
    pub manufacturer: String,
    pub model_name: String,
    pub model_number: String,
    pub model_description: String,
    pub serial_number: String,
    /// `http://10.0.0.10:8200` (bez zavrsne kose crte)
    pub base_url: String,
}

pub fn device_description(meta: &DeviceMeta) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<root xmlns="urn:schemas-upnp-org:device-1-0" xmlns:dlna="urn:schemas-dlna-org:device-1-0">
  <specVersion>
    <major>1</major>
    <minor>0</minor>
  </specVersion>
  <device>
    <deviceType>{device_type}</deviceType>
    <friendlyName>{friendly_name}</friendlyName>
    <manufacturer>{manufacturer}</manufacturer>
    <manufacturerURL>https://vaha.net</manufacturerURL>
    <modelDescription>{model_description}</modelDescription>
    <modelName>{model_name}</modelName>
    <modelNumber>{model_number}</modelNumber>
    <serialNumber>{serial_number}</serialNumber>
    <UDN>{udn}</UDN>
    <dlna:X_DLNADOC>DMS-1.50</dlna:X_DLNADOC>
    <presentationURL>{base_url}/</presentationURL>
    <!-- Ikone koje TV-i prikazuju u popisu uređaja (Serviio ih ima, mi ih nismo imali). -->
    <iconList>
      <icon>
        <mimetype>image/png</mimetype>
        <width>48</width>
        <height>48</height>
        <depth>24</depth>
        <url>{base_url}/icon-48.png</url>
      </icon>
      <icon>
        <mimetype>image/png</mimetype>
        <width>120</width>
        <height>120</height>
        <depth>24</depth>
        <url>{base_url}/icon-120.png</url>
      </icon>
      <icon>
        <mimetype>image/png</mimetype>
        <width>256</width>
        <height>256</height>
        <depth>24</depth>
        <url>{base_url}/icon-256.png</url>
      </icon>
      <icon>
        <mimetype>image/jpeg</mimetype>
        <dlna:profileID>JPEG_TN</dlna:profileID>
        <width>160</width>
        <height>160</height>
        <depth>24</depth>
        <url>{base_url}/icon-120.png</url>
      </icon>
    </iconList>
    <serviceList>
      <service>
        <serviceType>{content_directory}</serviceType>
        <serviceId>urn:upnp-org:serviceId:ContentDirectory</serviceId>
        <SCPDURL>/ContentDirectory/scpd.xml</SCPDURL>
        <controlURL>/ContentDirectory/control</controlURL>
        <eventSubURL>/ContentDirectory/event</eventSubURL>
      </service>
      <service>
        <serviceType>{connection_manager}</serviceType>
        <serviceId>urn:upnp-org:serviceId:ConnectionManager</serviceId>
        <SCPDURL>/ConnectionManager/scpd.xml</SCPDURL>
        <controlURL>/ConnectionManager/control</controlURL>
        <eventSubURL>/ConnectionManager/event</eventSubURL>
      </service>
    </serviceList>
  </device>
</root>
"#,
        device_type = DEVICE_TYPE,
        friendly_name = escape(&meta.friendly_name),
        manufacturer = escape(&meta.manufacturer),
        model_description = escape(&meta.model_description),
        model_name = escape(&meta.model_name),
        model_number = escape(&meta.model_number),
        serial_number = escape(&meta.serial_number),
        udn = escape(&meta.udn),
        base_url = escape(&meta.base_url),
        content_directory = SERVICE_CONTENT_DIRECTORY,
        connection_manager = SERVICE_CONNECTION_MANAGER,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scpd;

    fn meta() -> DeviceMeta {
        DeviceMeta {
            udn: "uuid:abc-123".to_string(),
            friendly_name: "Rustiio (mac)".to_string(), // zagrada mora proci escape nepromijenjena
            manufacturer: "Rustiio".to_string(),
            model_name: "Rustiio Media Server".to_string(),
            model_number: "0.1.0".to_string(),
            model_description: "DLNA/UPnP media server".to_string(),
            serial_number: "abc-123".to_string(),
            base_url: "http://10.0.0.10:8200".to_string(),
        }
    }

    #[test]
    fn description_contains_identity_and_services() {
        let xml = device_description(&meta());
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>"));
        assert!(xml.contains("<UDN>uuid:abc-123</UDN>"));
        assert!(xml.contains("<deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>"));
        assert!(xml.contains("<friendlyName>Rustiio (mac)</friendlyName>"));
        assert!(xml.contains("/ContentDirectory/control"));
        assert!(xml.contains("/ConnectionManager/control"));
        assert!(xml.contains("DMS-1.50"));
        assert!(xml.ends_with("</root>\n"));
    }

    #[test]
    fn scpd_urls_point_to_our_routes() {
        let xml = device_description(&meta());
        assert!(xml.contains("<SCPDURL>/ContentDirectory/scpd.xml</SCPDURL>"));
        assert!(!scpd::CONTENT_DIRECTORY_SCPD.is_empty());
        assert!(!scpd::CONNECTION_MANAGER_SCPD.is_empty());
    }

    #[test]
    fn friendly_name_with_specials_is_escaped() {
        let mut m = meta();
        m.friendly_name = "TV & <soba>".to_string();
        let xml = device_description(&m);
        assert!(xml.contains("<friendlyName>TV &amp; &lt;soba&gt;</friendlyName>"));
    }
}
