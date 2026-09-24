//! Model profila: capabilities uredjaja, transcode cilj i DLNA detalji.
//!
//! Sva polja imaju razumne defaulte, pa profil u TOML-u moze biti i pet redaka —
//! dopisuje se samo ono sto se razlikuje od "prosjecnog DLNA uredjaja".

use serde::{Deserialize, Serialize};

/// Zadani DLNA flags (streaming + background transfer) — isto sto salje Serviio.
pub const DEFAULT_FLAGS: &str = "01700000000000000000000000000000";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Po cemu prepoznajemo uredjaj.
    #[serde(rename = "match")]
    pub rules: MatchRules,
    pub video: VideoCaps,
    pub audio: AudioCaps,
    pub subtitles: SubtitleCaps,
    pub transcode: TranscodeTarget,
    pub dlna: DlnaCaps,
}

impl Profile {
    pub fn supports_container(&self, ext: &str) -> bool {
        let ext = normalize(ext);
        self.video.containers.iter().any(|value| normalize(value) == ext)
    }

    pub fn supports_video_codec(&self, codec: &str) -> bool {
        let codec = normalize(codec);
        self.video.codecs.iter().any(|value| normalize(value) == codec)
    }

    pub fn supports_audio_codec(&self, codec: &str) -> bool {
        let codec = normalize(codec);
        self.audio.codecs.iter().any(|value| normalize(value) == codec)
    }

    /// Moze li uredjaj prikazati ovu rezoluciju bez skaliranja.
    pub fn fits_video(&self, width: u32, height: u32) -> bool {
        width <= self.video.max_width && height <= self.video.max_height
    }

    pub fn supports_audio_track(&self, codec: &str, channels: u8) -> bool {
        self.supports_audio_codec(codec) && channels <= self.audio.max_channels
    }

    /// Podrzava li titlove i kojim nacinom.
    pub fn subtitle_mode(&self) -> SubtitleMode {
        self.subtitles.mode
    }
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: "generic".to_string(),
            name: "Generic DLNA".to_string(),
            description: "Uredjaj koji se nije dao prepoznati".to_string(),
            rules: MatchRules::default(),
            video: VideoCaps::default(),
            audio: AudioCaps::default(),
            subtitles: SubtitleCaps::default(),
            transcode: TranscodeTarget::default(),
            dlna: DlnaCaps::default(),
        }
    }
}

/// Pravila prepoznavanja (sva su substring matchovi, case-insensitive).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MatchRules {
    pub user_agent: Vec<String>,
    pub friendly_name: Vec<String>,
    pub device_type: Vec<String>,
    /// Tocne IP adrese — najjaci signal (za rucno dodijeljene uredjaje).
    pub ip: Vec<String>,
}

impl MatchRules {
    pub fn is_empty(&self) -> bool {
        self.user_agent.is_empty()
            && self.friendly_name.is_empty()
            && self.device_type.is_empty()
            && self.ip.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct VideoCaps {
    /// Kontejneri (ekstenzije) koje uredjaj sam otvara.
    pub containers: Vec<String>,
    /// Video kodeci (ffprobe `codec_name`) koje uredjaj dekodira.
    pub codecs: Vec<String>,
    pub max_width: u32,
    pub max_height: u32,
    pub max_bitrate_kbps: u32,
}

impl Default for VideoCaps {
    fn default() -> Self {
        Self {
            containers: ["mp4", "mkv", "ts", "m2ts", "mov", "avi"].iter().map(|s| s.to_string()).collect(),
            codecs: ["h264", "mpeg2video"].iter().map(|s| s.to_string()).collect(),
            max_width: 1920,
            max_height: 1080,
            max_bitrate_kbps: 20_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AudioCaps {
    pub codecs: Vec<String>,
    pub max_channels: u8,
}

impl Default for AudioCaps {
    fn default() -> Self {
        Self { codecs: ["aac", "mp3", "ac3"].iter().map(|s| s.to_string()).collect(), max_channels: 2 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleMode {
    /// Uredjaj ne prikazuje titlove — najbolje ih je upecati u sliku.
    None,
    /// Uredjaj sam ucita `res` s `text/srt` (najcesce).
    #[default]
    Soft,
    /// Titl se renderira u video (ffmpeg `subtitles=` filter).
    Burn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SubtitleCaps {
    pub mode: SubtitleMode,
    pub formats: Vec<String>,
}

impl Default for SubtitleCaps {
    fn default() -> Self {
        Self { mode: SubtitleMode::Soft, formats: ["srt", "vtt"].iter().map(|s| s.to_string()).collect() }
    }
}

/// Kako pripremiti stream kad uredjaj ne moze izvorni fajl.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TranscodeTarget {
    /// `mpegts` (najkompatibilniji) ili `mp4` (fragmented).
    pub container: String,
    pub video_codec: String,
    pub audio_codec: String,
    pub audio_channels: u8,
    pub max_bitrate_kbps: u32,
    /// Ako je zadano, visina se skalira na ovu vrijednost (npr. 720).
    pub max_height: Option<u32>,
    /// Dopusti remux (`-c copy`) kad su kodeci OK, a samo kontejner smeta.
    pub allow_remux: bool,
}

impl Default for TranscodeTarget {
    fn default() -> Self {
        Self {
            container: "mpegts".to_string(),
            video_codec: "h264".to_string(),
            audio_codec: "aac".to_string(),
            audio_channels: 2,
            max_bitrate_kbps: 12_000,
            max_height: None,
            allow_remux: true,
        }
    }
}

/// DLNA detalji koje neki uredjaji zahtijevaju.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DlnaCaps {
    /// `DLNA.ORG_OP` — prvi znak byte-seek, drugi time-seek ("01" = oba).
    pub op: String,
    pub flags: String,
    /// Neki uredjaji se zbune ako im se posalje `DLNA.ORG_PN` — ovdje ga gasimo.
    pub send_pn: bool,
    /// Uredjaj trazi `TimeSeekRange` umjesto `Range` (Samsung/Philips).
    pub time_seek: bool,
}

impl Default for DlnaCaps {
    fn default() -> Self {
        Self { op: "01".to_string(), flags: DEFAULT_FLAGS.to_string(), send_pn: true, time_seek: true }
    }
}

fn normalize(value: &str) -> String {
    value.trim().trim_start_matches('.').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_is_a_reasonable_tv() {
        let profile = Profile::default();
        assert_eq!(profile.id, "generic");
        assert!(profile.supports_container(".MKV"));
        assert!(profile.supports_video_codec("h264"));
        assert!(!profile.supports_video_codec("hevc"), "default TV ne dekodira HEVC");
        assert!(profile.supports_audio_track("ac3", 2));
        assert!(!profile.supports_audio_track("dts", 6));
        assert!(profile.fits_video(1920, 1080));
        assert!(!profile.fits_video(3840, 2160));
    }

    #[test]
    fn profile_roundtrips_through_toml() {
        let mut profile = Profile { id: "test".to_string(), ..Profile::default() };
        profile.video.codecs.push("hevc".to_string());
        profile.subtitles.mode = SubtitleMode::Burn;

        let text = toml::to_string_pretty(&profile).expect("serialize");
        let back: Profile = toml::from_str(&text).expect("deserialize");
        assert_eq!(back, profile);
    }

    #[test]
    fn minimal_toml_gets_defaults() {
        let text = r#"
id = "mini"
name = "Mini"
[match]
user_agent = ["MiniTV"]
"#;
        let profile: Profile = toml::from_str(text).expect("parse");
        assert_eq!(profile.rules.user_agent, vec!["MiniTV"]);
        assert_eq!(profile.transcode.container, "mpegts");
        assert_eq!(profile.transcode.video_codec, "h264");
        assert_eq!(profile.subtitles.mode, SubtitleMode::Soft);
        assert!(profile.rules.friendly_name.is_empty());
    }

    #[test]
    fn subtitle_mode_parses_from_toml() {
        let text = r#"
id = "burner"
name = "Burner"
[subtitles]
mode = "burn"
formats = ["srt"]
"#;
        let profile: Profile = toml::from_str(text).expect("parse");
        assert_eq!(profile.subtitle_mode(), SubtitleMode::Burn);
    }
}
