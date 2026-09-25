//! Spajanje profila uredjaja i transcode enginea: "sto poslati ovom TV-u".
//!
//! Sektor stoji u serveru jer je to jedino mjesto koje zna i profil (iz HTTP zaglavlja)
//! i metapodatke fajla (iz ffprobe-a) i stanje ffmpeg-a. CDS dobije samo gotovu putanju.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use rustiio_cds::{Playback, PlaybackResolver};
use rustiio_library::{MediaProbe, Node, NodeKind};
use rustiio_profiles::Profile;
use rustiio_transcode::hwaccel::HwSupport;
use rustiio_transcode::{Decision, PlaybackMode, decide};
use rustiio_upnp::escape_path_segment;

/// Odlucuje kako posluziti pojedini objekt za konkretan uredjaj.
///
/// Odluke se pamte po fajlu unutar jednog zahtjeva (Browse s 500 redova ne smije
/// racunati istu stvar dva puta), a metapodaci dolaze iz [`MediaProbe`] cachea.
pub struct PlaybackEngine<'a> {
    profile: &'a Profile,
    hw: &'a HwSupport,
    media: &'a MediaProbe,
    enabled: bool,
    what: String,
    decisions: Mutex<HashMap<PathBuf, Decision>>,
}

impl<'a> PlaybackEngine<'a> {
    pub fn new(
        profile: &'a Profile,
        hw: &'a HwSupport,
        media: &'a MediaProbe,
        enabled: bool,
        what: String,
    ) -> Self {
        Self { profile, hw, media, enabled, what, decisions: Mutex::new(HashMap::new()) }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn profile(&self) -> &Profile {
        self.profile
    }

    /// Putanja transcode resursa (`/tr/<id>/<ime>`).
    pub fn transcode_path(node: &Node) -> String {
        format!("/tr/{}/{}", node.id, escape_path_segment(&node.file_name()))
    }

    /// Putanja transcode resursa s ekstenzijom onoga što stvarno izlazi.
    ///
    /// Samsung (i drugi TV-i) gledaju **ekstenziju**, ne samo MIME: URL koji
    /// završava na `.mkv`, a nosi MPEG-TS stream, prijave kao „format nije
    /// podržan". Zato ime dobiva ekstenziju izlaznog kontejnera.
    pub fn transcode_path_for(node: &Node, container: &str) -> String {
        let ekstenzija = match container {
            "mpegts" | "ts" => "ts",
            "mp4" | "m4v" => "mp4",
            "matroska" | "webm" | "mkv" => "mkv",
            drugo => drugo,
        };
        let ime = node.file_name();
        let osnova = ime.rsplit_once('.').map(|(prije, _)| prije.to_string()).unwrap_or(ime);
        format!("/tr/{}/{}.{}", node.id, escape_path_segment(&osnova), ekstenzija)
    }

    /// Odluka za cvor; `None` ako nema metapodataka (tada se ide na direct play).
    pub fn decision(&self, node: &Node) -> Option<Decision> {
        if node.kind == NodeKind::Container || node.kind == NodeKind::Image {
            return None;
        }
        if let Ok(cache) = self.decisions.lock() {
            if let Some(hit) = cache.get(&node.path) {
                return Some(hit.clone());
            }
        }

        // Bez ffprobe podataka ne mozemo tvrditi da uredjaj ne moze original.
        let info = self.media.get(&node.path)?;
        let extension =
            node.path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let mut decision = decide(
            &info,
            self.profile,
            &extension,
            node.subtitles.iter().any(|staza| staza.is_text()),
            self.hw,
        );
        // `what` iz Settings: av = sve, audio = samo zvuk, video = samo slika.
        match (self.what.as_str(), &mut decision.mode) {
            ("audio", PlaybackMode::Transcode { video, audio }) => {
                if *video && !*audio {
                    decision.mode = PlaybackMode::Remux;
                } else {
                    *video = false;
                }
            }
            ("video", PlaybackMode::Transcode { audio, .. }) => *audio = false,
            _ => {}
        }

        if let Ok(mut cache) = self.decisions.lock() {
            cache.insert(node.path.clone(), decision.clone());
        }
        Some(decision)
    }

    /// Odluka + ljudski citljiv razlog (za log i REST).
    pub fn decision_summary(&self, node: &Node) -> Option<(String, String)> {
        let decision = self.decision(node)?;
        let mode = match &decision.mode {
            PlaybackMode::Direct => "direct".to_string(),
            PlaybackMode::Remux => format!("remux → {}", decision.container),
            PlaybackMode::Transcode { video, audio } => {
                let mut parts = Vec::new();
                if *video {
                    parts.push(format!(
                        "video → {} ({} kb/s, hw {})",
                        decision.video_encoder.as_deref().unwrap_or("?"),
                        decision.video_bitrate_kbps.unwrap_or(0),
                        decision.hw.name()
                    ));
                }
                if *audio {
                    parts.push(format!(
                        "audio → {} ({} kanala)",
                        decision.audio_encoder.as_deref().unwrap_or("?"),
                        decision.audio_channels.unwrap_or(2)
                    ));
                }
                format!("transcode ({})", parts.join(", "))
            }
        };
        Some((mode, decision.reasons.join("; ")))
    }
}

impl PlaybackResolver for PlaybackEngine<'_> {
    fn resolve(&self, node: &Node) -> Option<Playback> {
        if !self.enabled {
            return None;
        }
        let decision = self.decision(node)?;
        match decision.mode {
            // Uredjaj moze original — ne diramo nista.
            PlaybackMode::Direct => None,
            _ => Some(Playback {
                path: Self::transcode_path_for(node, &decision.container),
                protocol_info: decision.protocol_info.clone(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustiio_library::MediaInfo;
    use rustiio_profiles::VideoCaps;
    use std::path::Path;

    fn node(path: &str) -> Node {
        Node {
            id: "7".to_string(),
            parent_id: "1".to_string(),
            title: "Film".to_string(),
            kind: NodeKind::Video,
            path: PathBuf::from(path),
            size: 1000,
            modified: None,
            children: Vec::new(),
            subtitles: Vec::new(),
        }
    }

    fn hw() -> HwSupport {
        HwSupport::software()
    }

    fn limited_profile() -> Profile {
        let video = VideoCaps {
            containers: vec!["mp4".to_string()],
            codecs: vec!["h264".to_string()],
            ..VideoCaps::default()
        };
        Profile { id: "test".to_string(), video, ..Profile::default() }
    }

    fn hevc_mp4() -> MediaInfo {
        let video = rustiio_library::VideoStream {
            codec: "hevc".to_string(),
            width: 1920,
            height: 1080,
            bitrate_kbps: None,
            pix_fmt: None,
            profile: None,
            level: None,
        };
        MediaInfo { video: Some(video), ..MediaInfo::default() }
    }

    #[test]
    fn without_metadata_we_do_not_guess() {
        let media = MediaProbe::new("ffprobe", false);
        let profile = limited_profile();
        let hw = hw();
        let engine = PlaybackEngine::new(&profile, &hw, &media, true, "av".to_string());
        assert!(engine.resolve(&node("/media/film.mp4")).is_none(), "bez metapodataka = direct play");
    }

    #[test]
    fn unsupported_codec_gets_transcode_url() {
        let media = MediaProbe::new("ffprobe", false);
        media.remember(Path::new("/media/film.mp4"), Some(hevc_mp4()));
        let profile = limited_profile();
        let hw = hw();
        let engine = PlaybackEngine::new(&profile, &hw, &media, true, "av".to_string());

        let playback = engine.resolve(&node("/media/film.mp4")).expect("mora transcode");
        // Izlaz je MPEG-TS, pa i ime nosi `.ts` — Samsung gleda ekstenziju.
        assert_eq!(playback.path, "/tr/7/film.ts");
        assert!(playback.protocol_info.contains("video/mpeg"), "{}", playback.protocol_info);
    }

    #[test]
    fn disabled_transcode_always_direct_plays() {
        let media = MediaProbe::new("ffprobe", false);
        media.remember(Path::new("/media/film.mp4"), Some(hevc_mp4()));
        let profile = limited_profile();
        let hw = hw();
        let engine = PlaybackEngine::new(&profile, &hw, &media, false, "av".to_string());
        assert!(engine.resolve(&node("/media/film.mp4")).is_none());
    }
    #[test]
    fn transcode_url_nosi_ekstenziju_izlaznog_kontejnera() {
        // Samsung gleda ekstenziju: `.mkv` uz MPEG-TS stream = „format nije podrzan".
        let film = node("/filmovi/Film (2019).mkv");
        let ts = PlaybackEngine::transcode_path_for(&film, "mpegts");
        assert!(ts.starts_with("/tr/7/"), "{ts}");
        assert!(ts.ends_with(".ts"), "MPEG-TS izlaz mora imati .ts: {ts}");
        assert!(ts.contains("Film%20(2019)"), "ime se enkodira: {ts}");
        assert!(PlaybackEngine::transcode_path_for(&film, "mp4").ends_with(".mp4"));
        // Nepoznat kontejner se ne izmislja.
        assert!(PlaybackEngine::transcode_path_for(&film, "avi").ends_with(".avi"));
    }
}
