//! Metapodaci o medijskom fajlu (`ffprobe -print_format json`).
//!
//! Ovo je ulaz za decision engine: bez `codec_name`, rezolucije i broja kanala ne
//! mozemo znati treba li TV-u transcode. Proboj je namjerno odvojen od politike —
//! ovdje samo citamo sto u fajlu pise.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Sve sto o jednom fajlu znamo.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    /// `format_name` iz ffprobe-a (npr. `matroska,webm`, `mov,mp4,m4a,3gp,3g2,mj2`).
    pub container: String,
    pub duration_ms: Option<u64>,
    pub bitrate_kbps: Option<u32>,
    pub size_bytes: u64,
    pub video: Option<VideoStream>,
    /// Prvi audio (ono sto uredjaj najcesce pusta).
    pub audio: Option<AudioStream>,
    pub audio_streams: Vec<AudioStream>,
    pub embedded_subtitles: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoStream {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub bitrate_kbps: Option<u32>,
    pub pix_fmt: Option<String>,
    pub profile: Option<String>,
    pub level: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioStream {
    pub index: u32,
    pub codec: String,
    pub channels: u8,
    pub language: Option<String>,
    pub bitrate_kbps: Option<u32>,
}

impl MediaInfo {
    pub fn video_codec(&self) -> Option<&str> {
        self.video.as_ref().map(|video| video.codec.as_str())
    }

    pub fn audio_codec(&self) -> Option<&str> {
        self.audio.as_ref().map(|audio| audio.codec.as_str())
    }

    pub fn audio_channels(&self) -> u8 {
        self.audio.as_ref().map(|audio| audio.channels).unwrap_or(0)
    }

    pub fn resolution(&self) -> Option<(u32, u32)> {
        self.video.as_ref().map(|video| (video.width, video.height))
    }

    /// Kratki opis za log i UI.
    pub fn summary(&self) -> String {
        let video = match &self.video {
            Some(video) => format!("{} {}x{}", video.codec, video.width, video.height),
            None => "bez videa".to_string(),
        };
        let audio = match &self.audio {
            Some(audio) => format!("{}/{}ch", audio.codec, audio.channels),
            None => "bez zvuka".to_string(),
        };
        let bitrate = self.bitrate_kbps.map(|value| format!(" {value} kbps")).unwrap_or_default();
        format!("{video} + {audio}{bitrate} ({})", self.container)
    }

    pub fn from_json(text: &str) -> Option<Self> {
        let raw: FfprobeOutput = serde_json::from_str(text).ok()?;
        Some(Self::from_ffprobe(raw))
    }

    fn from_ffprobe(raw: FfprobeOutput) -> Self {
        let mut audio_streams = Vec::new();
        let mut video = None;
        let mut embedded_subtitles = 0;

        for stream in &raw.streams {
            match stream.codec_type.as_deref() {
                Some("video") => {
                    if video.is_none() {
                        video = Some(VideoStream {
                            codec: stream.codec_name.clone().unwrap_or_default(),
                            width: stream.width.unwrap_or(0),
                            height: stream.height.unwrap_or(0),
                            bitrate_kbps: parse_number(stream.bit_rate.as_deref()),
                            pix_fmt: stream.pix_fmt.clone(),
                            profile: stream.profile.clone(),
                            level: stream.level,
                        });
                    }
                }
                Some("audio") => audio_streams.push(AudioStream {
                    index: stream.index.unwrap_or(0),
                    codec: stream.codec_name.clone().unwrap_or_default(),
                    channels: stream.channels.unwrap_or(0),
                    language: stream.tags.as_ref().and_then(|tags| tags.language.clone()),
                    bitrate_kbps: parse_number(stream.bit_rate.as_deref()),
                }),
                Some("subtitle") => embedded_subtitles += 1,
                _ => {}
            }
        }

        let format = raw.format.unwrap_or_default();
        let duration_ms = format.duration.as_deref().and_then(parse_seconds_to_ms);
        let size_bytes = format.size.as_deref().and_then(|value| value.parse::<u64>().ok()).unwrap_or(0);
        let bitrate_kbps = format
            .bit_rate
            .as_deref()
            .and_then(|value| value.parse::<u64>().ok())
            .map(|value| (value / 1000) as u32)
            .or_else(|| {
                size_bytes.checked_mul(8).zip(duration_ms).and_then(|(bits, ms)| {
                    if ms == 0 { None } else { Some((bits as u128 * 1000 / ms as u128 / 1000) as u32) }
                })
            });

        Self {
            container: format.format_name.unwrap_or_default(),
            duration_ms,
            bitrate_kbps,
            size_bytes,
            audio: audio_streams.first().cloned(),
            audio_streams,
            video,
            embedded_subtitles,
        }
    }
}

/// Pokreni ffprobe nad fajlom (blokirajuce — zovi iz `spawn_blocking`).
pub fn probe(path: &Path, ffprobe_path: &str) -> Option<MediaInfo> {
    let output = Command::new(ffprobe_path)
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            "-show_entries",
            "stream=index,codec_type,codec_name,width,height,channels,bit_rate,profile,level,pix_fmt:stream_tags=language:format=format_name,duration,size,bit_rate",
        ])
        .arg(path)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            MediaInfo::from_json(&String::from_utf8_lossy(&output.stdout))
        }
        Ok(output) => {
            debug!(path = %path.display(), stderr = %String::from_utf8_lossy(&output.stderr).trim(), "ffprobe nije procitao fajl");
            None
        }
        Err(err) => {
            warn!(program = ffprobe_path, error = %err, "ffprobe se ne moze pokrenuti");
            None
        }
    }
}

/// Cache ffprobe metapodataka.
///
/// Decision engine mora znati kodek svakog fajla prije nego sto TV-u kaze "ovo mozes
/// pustiti". Pokretanje ffprobe-a za svaki red u Browse-u je presporo, zato se
/// rezultat pamti (i uspjeh i neuspjeh); cache se puni u pozadini nakon skena.
pub struct MediaProbe {
    ffprobe: String,
    enabled: bool,
    cache: RwLock<HashMap<PathBuf, Option<MediaInfo>>>,
}

impl MediaProbe {
    pub fn new(ffprobe: impl Into<String>, enabled: bool) -> Self {
        Self { ffprobe: ffprobe.into(), enabled, cache: RwLock::new(HashMap::new()) }
    }

    /// Je li fajl vec proban (bilo uspjesno ili neuspjesno)?
    pub fn is_known(&self, path: &Path) -> bool {
        self.cache.read().map(|cache| cache.contains_key(path)).unwrap_or(false)
    }

    /// Metapodaci iz cachea (ne pokrece ffprobe).
    pub fn get(&self, path: &Path) -> Option<MediaInfo> {
        self.cache.read().ok()?.get(path).cloned().flatten()
    }

    /// Ocitaj i zapamti (blokirajuce — zovi iz `spawn_blocking`).
    pub fn probe(&self, path: &Path) -> Option<MediaInfo> {
        if let Some(hit) = self.cache.read().ok().and_then(|cache| cache.get(path).cloned()) {
            return hit;
        }
        let info = if self.enabled { probe(path, &self.ffprobe) } else { None };
        if let Ok(mut cache) = self.cache.write() {
            if cache.len() >= 50_000 {
                cache.clear();
            }
            cache.insert(path.to_path_buf(), info.clone());
        }
        info
    }

    /// Upiši metapodatke bez pozivanja ffprobe-a (pozadinsko punjenje cachea, testovi).
    pub fn remember(&self, path: &Path, info: Option<MediaInfo>) {
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(path.to_path_buf(), info);
        }
    }

    pub fn forget(&self, path: &Path) {
        if let Ok(mut cache) = self.cache.write() {
            cache.remove(path);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.cache.read().map(|cache| cache.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn parse_number(value: Option<&str>) -> Option<u32> {
    let parsed = value?.trim().parse::<u64>().ok()?;
    Some((parsed / 1000) as u32)
}

fn parse_seconds_to_ms(value: &str) -> Option<u64> {
    let seconds: f64 = value.trim().parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some((seconds * 1000.0).round() as u64)
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    #[serde(default)]
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Default, Deserialize)]
struct FfprobeFormat {
    format_name: Option<String>,
    duration: Option<String>,
    size: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FfprobeStream {
    index: Option<u32>,
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u8>,
    bit_rate: Option<String>,
    profile: Option<String>,
    level: Option<i32>,
    pix_fmt: Option<String>,
    tags: Option<FfprobeTags>,
}

#[derive(Debug, Default, Deserialize)]
struct FfprobeTags {
    language: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "streams": [
        {"index": 0, "codec_name": "hevc", "codec_type": "video", "width": 1920, "height": 1080,
         "pix_fmt": "yuv420p", "profile": "Main 10", "level": 120, "bit_rate": "4500000"},
        {"index": 1, "codec_name": "eac3", "codec_type": "audio", "channels": 6,
         "bit_rate": "640000", "tags": {"language": "eng"}},
        {"index": 2, "codec_name": "ac3", "codec_type": "audio", "channels": 2,
         "tags": {"language": "hrv"}},
        {"index": 3, "codec_name": "subrip", "codec_type": "subtitle"}
      ],
      "format": {"format_name": "matroska,webm", "duration": "5400.523000", "size": "3214567890", "bit_rate": "4760000"}
    }"#;

    #[test]
    fn parses_hevc_mkv_with_multiple_audio_tracks() {
        let info = MediaInfo::from_json(SAMPLE).expect("parse");
        assert_eq!(info.container, "matroska,webm");
        assert_eq!(info.duration_ms, Some(5_400_523));
        assert_eq!(info.size_bytes, 3_214_567_890);
        assert_eq!(info.bitrate_kbps, Some(4760));

        let video = info.video.as_ref().expect("video");
        assert_eq!(video.codec, "hevc");
        assert_eq!((video.width, video.height), (1920, 1080));
        assert_eq!(video.bitrate_kbps, Some(4500));
        assert_eq!(video.profile.as_deref(), Some("Main 10"));

        assert_eq!(info.audio_streams.len(), 2);
        let audio = info.audio.as_ref().expect("prvi audio");
        assert_eq!(audio.codec, "eac3");
        assert_eq!(audio.channels, 6);
        assert_eq!(audio.language.as_deref(), Some("eng"));
        assert_eq!(info.embedded_subtitles, 1);
        assert_eq!(info.audio_channels(), 6);
        assert!(info.summary().contains("hevc 1920x1080"));
    }

    #[test]
    fn rejects_junk_and_empty_json() {
        assert!(MediaInfo::from_json("nije json").is_none());
        assert!(MediaInfo::from_json("").is_none());

        let empty = MediaInfo::from_json(r#"{"streams": []}"#).expect("prazan ali valjan");
        assert!(empty.video.is_none());
        assert!(empty.audio.is_none());
        assert_eq!(empty.bitrate_kbps, None);
        assert_eq!(empty.container, "");
    }

    #[test]
    fn bitrate_is_estimated_when_format_bit_rate_missing() {
        let text = r#"{
          "streams": [{"index": 0, "codec_name": "h264", "codec_type": "video", "width": 1280, "height": 720}],
          "format": {"format_name": "matroska,webm", "duration": "10.0", "size": "1000000"}
        }"#;
        let info = MediaInfo::from_json(text).expect("parse");
        // 1 MB u 10 s = 800 kbps
        assert_eq!(info.bitrate_kbps, Some(800));
    }

    #[test]
    fn handles_missing_duration_without_panicking() {
        let text = r#"{"streams": [], "format": {"format_name": "matroska,webm", "size": "1000000"}}"#;
        let info = MediaInfo::from_json(text).expect("parse");
        assert_eq!(info.duration_ms, None);
        assert_eq!(info.bitrate_kbps, None);
    }

    #[test]
    fn language_tags_are_read_per_stream() {
        let info = MediaInfo::from_json(SAMPLE).unwrap();
        let languages: Vec<Option<String>> =
            info.audio_streams.iter().map(|audio| audio.language.clone()).collect();
        assert_eq!(languages, vec![Some("eng".to_string()), Some("hrv".to_string())]);
    }

    /// Stvarni ffprobe nad stvarnim fajlom — preskace se ako alati nisu dostupni.
    #[test]
    fn media_probe_caches_both_success_and_failure() {
        let probe = MediaProbe::new("ffprobe-definitivno-ne-postoji", true);
        assert!(!probe.is_known(Path::new("/tmp/x.mkv")));
        assert_eq!(probe.probe(Path::new("/tmp/x.mkv")), None);
        assert!(probe.is_known(Path::new("/tmp/x.mkv")), "i neuspjeh se pamti");
        assert_eq!(probe.len(), 1);
        probe.forget(Path::new("/tmp/x.mkv"));
        assert_eq!(probe.len(), 0);
    }

    #[test]
    fn disabled_media_probe_never_runs_ffprobe() {
        let probe = MediaProbe::new("ffprobe", false);
        assert_eq!(probe.probe(Path::new("/tmp/nema.mkv")), None);
        assert!(probe.is_known(Path::new("/tmp/nema.mkv")));
    }

    /// Stvarni ffprobe nad stvarnim fajlom — preskace se ako alati nisu dostupni.
    #[test]
    fn real_ffprobe_reads_generated_file() {
        let dir = std::env::temp_dir().join(format!("rustiio-mediainfo-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("test.mp4");
        let generated = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=10:duration=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(&file)
            .status();
        if !matches!(generated, Ok(status) if status.success()) {
            eprintln!("ffmpeg nije dostupan — preskacem");
            return;
        }

        let info = probe(&file, "ffprobe").expect("ffprobe procitao fajl");
        assert_eq!(info.video_codec(), Some("h264"));
        assert_eq!(info.audio_codec(), Some("aac"));
        assert_eq!(info.resolution(), Some((320, 240)));
        assert!((1_800..=2_200).contains(&info.duration_ms.unwrap_or(0)), "trajanje ~2 s: {info:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
