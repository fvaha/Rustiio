//! UPnP sloj — sve sto putuje preko zice osim samih medijskih bajtova.
//!
//! - [`device`] — `device.xml` (opis uredaja)
//! - [`scpd`] — opisi servisa (ContentDirectory, ConnectionManager)
//! - [`soap`] — SOAP envelope: parsiranje zahtjeva i gradnja odgovora
//! - [`didl`] — DIDL-Lite: ono sto TV zapravo prikazuje kao popis
//! - [`protocol`] — `protocolInfo` / `contentFeatures` po ekstenziji

pub mod device;
pub mod didl;
pub mod protocol;
pub mod scpd;
pub mod soap;

pub use device::{DeviceMeta, device_description};
pub use didl::{Object, Resource, render as render_didl};
pub use protocol::ProtocolInfo;

/// XML escape za tekst i atribute.
///
/// Radi vlastito umjesto `quick_xml::escape` da bismo kontrolirali i kontrolne
/// znakove (XML 1.0 ih zabranjuje, a imena fajlova ih znaju sadrzavati).
pub fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Percent-encodiranje jednog segmenta puta (za URL medija s razmacima i slovima).
pub fn escape_path_segment(input: &str) -> String {
    use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

    const SEGMENT: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'#')
        .add(b'%')
        .add(b'&')
        .add(b'+')
        .add(b'/')
        .add(b':')
        .add(b'?')
        .add(b'<')
        .add(b'>')
        .add(b'\\')
        .add(b'^')
        .add(b'`')
        .add(b'{')
        .add(b'|')
        .add(b'}');

    utf8_percent_encode(input, SEGMENT).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_handles_xml_specials() {
        assert_eq!(escape("Film & <Serija> \"2\""), "Film &amp; &lt;Serija&gt; &quot;2&quot;");
        assert_eq!(escape("normalno ime.mkv"), "normalno ime.mkv");
    }

    #[test]
    fn escape_drops_illegal_control_chars() {
        assert_eq!(escape("a\u{0}b"), "ab");
    }

    #[test]
    fn path_segment_escapes_spaces_and_keeps_dots() {
        assert_eq!(escape_path_segment("The Movie (2019).mkv"), "The%20Movie%20(2019).mkv");
    }
}
