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
    decisions: Mutex<HashMap<PathBuf, Decision>>,
}

impl<'a> PlaybackEngine<'a> {
    pub fn new(profile: &'a Profile, hw: &'a HwSupport, media: &'a MediaProbe, enabled: bool) -> Self {
        Self { profile, hw, media, enabled, decisions: Mutex::new(HashMap::new()) }
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
        let decision = decide(&info, self.profile, &extension, node.subtitle.is_some(), self.hw);

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
                path: Self::transcode_path(node),
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
            subtitle: None,
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
        let engine = PlaybackEngine::new(&profile, &hw, &media, true);
        assert!(engine.resolve(&node("/media/film.mp4")).is_none(), "bez metapodataka = direct play");
    }

    #[test]
    fn unsupported_codec_gets_transcode_url() {
        let media = MediaProbe::new("ffprobe", false);
        media.remember(Path::new("/media/film.mp4"), Some(hevc_mp4()));
        let profile = limited_profile();
        let hw = hw();
        let engine = PlaybackEngine::new(&profile, &hw, &media, true);

        let playback = engine.resolve(&node("/media/film.mp4")).expect("mora transcode");
        assert_eq!(playback.path, "/tr/7/film.mp4");
        assert!(playback.protocol_info.contains("video/mp2t"), "{}", playback.protocol_info);
    }

    #[test]
    fn disabled_transcode_always_direct_plays() {
        let media = MediaProbe::new("ffprobe", false);
        media.remember(Path::new("/media/film.mp4"), Some(hevc_mp4()));
        let profile = limited_profile();
        let hw = hw();
        let engine = PlaybackEngine::new(&profile, &hw, &media, false);
        assert!(engine.resolve(&node("/media/film.mp4")).is_none());
    }
}
