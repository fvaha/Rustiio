//! `protocolInfo` i `contentFeatures` — DLNA "osobna karta" svakog resursa.
//!
//! Format (4. polje je opcionalni dodatak):
//! `http-get:*:<mime>:DLNA.ORG_PN=<profil>;DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=...`
//!
//! `DLNA.ORG_PN` navodimo samo kad smo sigurni u profil (MP4/H.264/AAC). Za MKV,
//! AVI i slicno TV-u dajemo goli MIME i pustimo ga da sam odluci — tako se izbjegne
//! klasican problem "TV odbije jer mu PN ne odgovara".

/// Streaming + background transfer (ono sto Serviio salje).
pub const DEFAULT_FLAGS: &str = "01700000000000000000000000000000";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolInfo {
    pub mime: String,
    /// DLNA.ORG_PN profil (npr. `AVC_MP4_MP_HD_1080i_AAC`).
    pub pn: Option<String>,
    pub op: Option<String>,
    pub ci: Option<String>,
    pub flags: Option<String>,
}

impl ProtocolInfo {
    pub fn new(mime: &str) -> Self {
        Self {
            mime: mime.to_string(),
            pn: None,
            op: Some("01".to_string()),
            ci: Some("0".to_string()),
            flags: Some(DEFAULT_FLAGS.to_string()),
        }
    }

    pub fn with_pn(mut self, pn: &str) -> Self {
        self.pn = Some(pn.to_string());
        self
    }

    pub fn with_op(mut self, op: &str) -> Self {
        self.op = Some(op.to_string());
        self
    }

    pub fn with_flags(mut self, flags: &str) -> Self {
        self.flags = Some(flags.to_string());
        self
    }

    /// Bez `DLNA.ORG_PN` — neki uredjaji (Xbox, PlayStation) se na njega zbune.
    pub fn without_pn(mut self) -> Self {
        self.pn = None;
        self
    }

    /// Bez ikakvih DLNA dodataka — samo `http-get:*:mime:*`.
    pub fn bare(mime: &str) -> Self {
        Self { mime: mime.to_string(), pn: None, op: None, ci: None, flags: None }
    }

    /// Puno `protocolInfo` za DIDL `res@protocolInfo`.
    pub fn to_protocol_info(&self) -> String {
        let extra = self.content_features();
        if extra == "*" {
            format!("http-get:*:{}:*", self.mime)
        } else {
            format!("http-get:*:{}:{}", self.mime, extra)
        }
    }

    /// Vrijednost `contentFeatures.dlna.org` headera.
    pub fn content_features(&self) -> String {
        let mut parts = Vec::new();
        if let Some(pn) = &self.pn {
            parts.push(format!("DLNA.ORG_PN={pn}"));
        }
        if let Some(op) = &self.op {
            parts.push(format!("DLNA.ORG_OP={op}"));
        }
        if let Some(ci) = &self.ci {
            parts.push(format!("DLNA.ORG_CI={ci}"));
        }
        if let Some(flags) = &self.flags {
            parts.push(format!("DLNA.ORG_FLAGS={flags}"));
        }
        if parts.is_empty() { "*".to_string() } else { parts.join(";") }
    }
}

/// MIME po ekstenziji (malim slovima, bez tocke).
pub fn mime_for_ext(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mov" => "video/quicktime",
        "ts" | "m2ts" | "mts" => "video/vnd.dlna.mpeg-tts",
        "mpg" | "mpeg" | "vob" => "video/mpeg",
        "wmv" | "asf" => "video/x-ms-wmv",
        "flv" => "video/x-flv",
        "divx" => "video/x-msvideo",
        "3gp" => "video/3gpp",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "srt" => "text/srt",
        "vtt" => "text/vtt",
        "sub" => "text/plain",
        "ass" | "ssa" => "text/x-ssa",
        _ => "application/octet-stream",
    }
}

/// procjena DLNA profila s vrijednostima iz profila uredjaja.
pub fn guess_for_ext_with(ext: &str, op: &str, flags: &str, send_pn: bool) -> ProtocolInfo {
    let mut info = guess_for_ext(ext);
    if !send_pn {
        info.pn = None;
    }
    if !op.trim().is_empty() {
        info.op = Some(op.trim().to_string());
    }
    if !flags.trim().is_empty() {
        info.flags = Some(flags.trim().to_string());
    }
    info
}

/// Najbolja procjena DLNA profila za zadanu ekstenziju.
pub fn guess_for_ext(ext: &str) -> ProtocolInfo {
    let mime = mime_for_ext(ext);
    match ext.to_ascii_lowercase().as_str() {
        // MP4 s H.264/AAC je jedino gdje PN smijemo tvrditi bez ffprobe-a.
        "mp4" | "m4v" => ProtocolInfo::new(mime).with_pn("AVC_MP4_MP_HD_1080i_AAC"),
        // Zvuk i titlovi nemaju korisnih PN-ova.
        "mp3" | "m4a" | "aac" | "flac" | "wav" | "ogg" | "oga" | "srt" | "vtt" | "sub" | "ass" | "ssa"
        | "jpg" | "jpeg" | "png" => ProtocolInfo::bare(mime),
        // Ostali video: MIME bez PN-a, TV odlucuje.
        _ => ProtocolInfo::new(mime),
    }
}

/// Svi `protocolInfo` stringovi koje oglasavamo u `ConnectionManager.GetProtocolInfo`.
pub fn source_protocol_info(extensions: &[String]) -> String {
    let mut seen: Vec<String> = Vec::new();
    for ext in extensions {
        let pi = guess_for_ext(ext).to_protocol_info();
        if !seen.contains(&pi) {
            seen.push(pi);
        }
    }
    for extra in ["srt", "vtt", "jpg", "png"] {
        let pi = guess_for_ext(extra).to_protocol_info();
        if !seen.contains(&pi) {
            seen.push(pi);
        }
    }
    seen.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp4_gets_profile_but_mkv_stays_open() {
        let mp4 = guess_for_ext("mp4");
        assert!(mp4.to_protocol_info().starts_with("http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_MP"));
        assert!(mp4.content_features().contains("DLNA.ORG_OP=01"));

        let mkv = guess_for_ext("mkv");
        assert_eq!(
            mkv.to_protocol_info(),
            "http-get:*:video/x-matroska:DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000"
        );
        assert!(!mkv.content_features().contains("DLNA.ORG_PN"));
    }

    #[test]
    fn subtitles_are_bare_protocol_info() {
        assert_eq!(guess_for_ext("srt").to_protocol_info(), "http-get:*:text/srt:*");
        assert_eq!(guess_for_ext("SRT").to_protocol_info(), "http-get:*:text/srt:*");
    }

    #[test]
    fn mime_for_unknown_is_octet_stream() {
        assert_eq!(mime_for_ext("xyz"), "application/octet-stream");
    }

    #[test]
    fn protocol_info_list_is_deduped_and_nonempty() {
        let exts: Vec<String> = ["mkv", "mp4", "srt", "mkv"].iter().map(|s| s.to_string()).collect();
        let list = source_protocol_info(&exts);
        assert!(list.contains("video/x-matroska"));
        assert_eq!(list.matches("video/x-matroska").count(), 1, "duplikati se filtriraju");
        assert!(list.contains("text/srt"));
    }
}
