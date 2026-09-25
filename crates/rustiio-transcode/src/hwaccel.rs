//! Detekcija hardverskog ubrzanja — testnim enkodiranjem, ne nagadjanjem.
//!
//! "Ima li NVENC" se ne moze saznati iz `ffmpeg -encoders` (napisano je i kad GPU nema
//! ili driver ne radi). Zato svaki kandidat dobije 1 sekundu testnog izvora: ako
//! ffmpeg vrati 0, ubrzanje stvarno radi.

use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HwAccel {
    None,
    Nvenc,
    Vaapi,
    Qsv,
    #[serde(rename = "videotoolbox")]
    VideoToolbox,
    Amf,
}

impl HwAccel {
    pub fn name(self) -> &'static str {
        match self {
            HwAccel::None => "softver (libx264)",
            HwAccel::Nvenc => "NVIDIA NVENC",
            HwAccel::Vaapi => "VAAPI",
            HwAccel::Qsv => "Intel Quick Sync",
            HwAccel::VideoToolbox => "Apple VideoToolbox",
            HwAccel::Amf => "AMD AMF",
        }
    }

    /// Iz configa (`auto`, `none`, `nvenc`, `vaapi`, ...).
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "off" | "software" | "softver" => Some(HwAccel::None),
            "nvenc" | "nvidia" => Some(HwAccel::Nvenc),
            "vaapi" => Some(HwAccel::Vaapi),
            "qsv" | "quicksync" => Some(HwAccel::Qsv),
            "videotoolbox" | "vtb" | "apple" => Some(HwAccel::VideoToolbox),
            "amf" | "amd" => Some(HwAccel::Amf),
            _ => None,
        }
    }

    /// Iz imena ffmpeg enkodera (`h264_nvenc`, `hevc_vaapi`, `libx264`) u obitelj.
    pub fn from_encoder(encoder: &str) -> Self {
        let name = encoder.trim().to_ascii_lowercase();
        for (suffix, hw) in [
            ("_nvenc", HwAccel::Nvenc),
            ("_vaapi", HwAccel::Vaapi),
            ("_qsv", HwAccel::Qsv),
            ("_videotoolbox", HwAccel::VideoToolbox),
            ("_amf", HwAccel::Amf),
        ] {
            if name.ends_with(suffix) {
                return hw;
            }
        }
        HwAccel::None
    }
}

/// Sto je na ovom stroju stvarno dostupno.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HwSupport {
    pub available: Vec<HwAccel>,
    pub preferred: HwAccel,
    /// Zapis za log/UI (npr. "nvenc: driver ne radi").
    pub notes: Vec<String>,
    /// Ima li ffmpeg `subtitles` filter (libass) — bez toga burn-in nije moguc.
    pub subtitles_filter: bool,
    /// Enkoder koji je korisnik izričito odabrao (`h264_nvenc`, `libx264`, ...).
    /// Prazno = pusti odluku da izabere prema kodeku i hardveru.
    pub encoder: Option<String>,
    /// Niti za softverski enkoder (0 = ne diraj ffmpeg).
    pub threads: u32,
    /// Dekodiraj hardverski i kad enkodira procesor.
    pub hardware_decode: bool,
}

/// Podešavanja iz `[transcode]` koja utječu na enkodiranje.
#[derive(Debug, Clone, Default)]
pub struct Tuning {
    pub encoder: Option<String>,
    pub threads: u32,
    pub hardware_decode: bool,
}

impl HwSupport {
    pub fn software() -> Self {
        Self {
            available: Vec::new(),
            preferred: HwAccel::None,
            notes: vec!["nema HW ubrzanja".to_string()],
            subtitles_filter: false,
            encoder: None,
            threads: 0,
            hardware_decode: true,
        }
    }

    /// Mozemo li upecati titl u sliku? (Ako ne, titl ide kao zaseban resurs.)
    pub fn can_burn_subtitles(&self) -> bool {
        self.subtitles_filter
    }

    /// Kratak opis hardverskog ubrzanja (za log i REST).
    pub fn summary(&self) -> String {
        if self.available.is_empty() {
            return "softverski (bez GPU ubrzanja)".to_string();
        }
        format!(
            "{} (odabrano: {})",
            self.available.iter().map(|hw| hw.name()).collect::<Vec<_>>().join(", "),
            self.preferred.name()
        )
    }

    pub fn supports(&self, hw: HwAccel) -> bool {
        hw == HwAccel::None || self.available.contains(&hw)
    }

    /// Upisi odabir iz configa (konkretan enkoder, niti, HW dekodiranje).
    pub fn apply_tuning(&mut self, tuning: &Tuning) {
        self.encoder = tuning.encoder.clone().filter(|value| !value.trim().is_empty());
        self.threads = tuning.threads;
        self.hardware_decode = tuning.hardware_decode;
    }

    /// Je li odabrani enkoder softverski (niti tada imaju smisla)?
    pub fn encoder_is_software(&self) -> bool {
        match &self.encoder {
            Some(name) => HwAccel::from_encoder(name) == HwAccel::None,
            None => self.preferred == HwAccel::None,
        }
    }
}

/// Redoslijed kojim biramo kad je u configu `auto` (NVENC prvi — najcesce na .10).
const AUTO_ORDER: [HwAccel; 5] =
    [HwAccel::Nvenc, HwAccel::Qsv, HwAccel::Vaapi, HwAccel::VideoToolbox, HwAccel::Amf];

/// Detektiraj dostupno ubrzanje; `requested` je iz configa (`auto` = probaj sve).
pub fn detect(ffmpeg_path: &str, requested: &str) -> HwSupport {
    let subtitles_filter = has_subtitles_filter(ffmpeg_path);
    if !subtitles_filter {
        warn!(
            "ffmpeg nema 'subtitles' filter (libass) — burn-in titlova nece raditi, titlovi idu kao zaseban resurs"
        );
    }

    if !probe_binary(ffmpeg_path) {
        return HwSupport {
            available: Vec::new(),
            preferred: HwAccel::None,
            notes: vec![format!("{ffmpeg_path} nije pokrenut — transcode ide u softver")],
            subtitles_filter,
            encoder: None,
            threads: 0,
            hardware_decode: false,
        };
    }

    let requested = requested.trim().to_ascii_lowercase();
    if requested == "none" || requested == "off" {
        return HwSupport {
            available: Vec::new(),
            preferred: HwAccel::None,
            notes: vec!["HW ubrzanje iskljuceno u configu".to_string()],
            subtitles_filter,
            encoder: None,
            threads: 0,
            hardware_decode: false,
        };
    }

    let mut available = Vec::new();
    let mut notes = Vec::new();

    let candidates: Vec<HwAccel> = match HwAccel::parse(&requested) {
        Some(HwAccel::None) => Vec::new(),
        Some(explicit) => vec![explicit],
        None => AUTO_ORDER.to_vec(),
    };

    for candidate in candidates {
        match test_encoder(ffmpeg_path, candidate) {
            Ok(()) => {
                info!(hw = candidate.name(), "hardversko ubrzanje dostupno");
                available.push(candidate);
            }
            Err(reason) => {
                debug!(hw = candidate.name(), reason = %reason, "HW ubrzanje nije dostupno");
                notes.push(format!("{}: {reason}", candidate.name()));
            }
        }
    }

    let preferred = if requested == "auto" || requested.is_empty() {
        available.first().copied().unwrap_or(HwAccel::None)
    } else {
        // Korisnik je trazio konkretno; ako ne radi, pada na softver.
        HwAccel::parse(&requested).filter(|hw| available.contains(hw)).unwrap_or(HwAccel::None)
    };

    if preferred == HwAccel::None {
        notes.push("transcode ide u softver (libx264)".to_string());
    }

    HwSupport {
        available,
        preferred,
        notes,
        subtitles_filter,
        encoder: None,
        threads: 0,
        hardware_decode: true,
    }
}

/// Ima li ffmpeg `subtitles` filter (libass)? Bez njega burn-in ne moze raditi.
fn has_subtitles_filter(ffmpeg_path: &str) -> bool {
    std::process::Command::new(ffmpeg_path)
        .args(["-hide_banner", "-filters"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.split_whitespace().any(|token| token == "subtitles"))
        })
        .unwrap_or(false)
}

/// Ime ffmpeg encodera za zadani kodek i ubrzanje.
/// Zastavice za hardversko DEKODIRANJE (ispred `-i`).
///
/// Namjerno bez `-hwaccel_output_format`: okviri se vraćaju u sistemsku memoriju,
/// pa radi u svakoj kombinaciji (GPU dekodira + procesor enkodira = "kombinirano").
pub fn decode_args(hw: HwAccel, encoder: Option<&str>) -> Option<Vec<String>> {
    let obitelj = match encoder {
        Some(name) if HwAccel::from_encoder(name) != HwAccel::None => HwAccel::from_encoder(name),
        _ => hw,
    };
    let device = |args: &mut Vec<String>| {
        // VAAPI treba i izlazni uređaj prije ulaza.
        if let Some(cvor) = vaapi_device() {
            if !args.iter().any(|value| value == "-vaapi_device") {
                args.push("-vaapi_device".to_string());
                args.push(cvor);
            }
        }
    };
    let mut args = Vec::new();
    match obitelj {
        HwAccel::Nvenc => args.extend(["-hwaccel".to_string(), "cuda".to_string()]),
        HwAccel::Qsv => args.extend(["-hwaccel".to_string(), "qsv".to_string()]),
        HwAccel::Vaapi => {
            device(&mut args);
            args.extend(["-hwaccel".to_string(), "vaapi".to_string()]);
        }
        HwAccel::VideoToolbox => args.extend(["-hwaccel".to_string(), "videotoolbox".to_string()]),
        HwAccel::Amf => args.extend(["-hwaccel".to_string(), "d3d11va".to_string()]),
        HwAccel::None => return None,
    }
    (!args.is_empty()).then_some(args)
}

/// Prvi VAAPI uređaj na stroju.
pub fn vaapi_device() -> Option<String> {
    let mut cvorovi: Vec<String> = std::fs::read_dir("/dev/dri")
        .ok()?
        .flatten()
        .map(|entry| format!("/dev/dri/{}", entry.file_name().to_string_lossy()))
        .filter(|path| path.contains("renderD"))
        .collect();
    cvorovi.sort();
    cvorovi.into_iter().next()
}

pub fn video_encoder(hw: HwAccel, codec: &str) -> Option<&'static str> {
    let codec = codec.to_ascii_lowercase();
    // Ciljani kodek koji hardver ne zna (npr. MPEG-2 za Samsung) mora dobiti
    // softverski enkoder — inače bi ga donji `_` ogranak prebacio u H.264 i
    // stream ne bi odgovarao profilu (Samsung u MPEG-TS-u traži baš MPEG-2).
    match codec.as_str() {
        "mpeg2video" | "mpeg2" => return Some("mpeg2video"),
        "mpeg4" => return Some("mpeg4"),
        "vp8" => return Some("libvpx"),
        "vp9" => return Some("libvpx-vp9"),
        "h264" | "avc" | "hevc" | "h265" => {}
        _ => {}
    }
    let (h264, hevc) = match (codec.as_str(), hw) {
        ("hevc" | "h265", HwAccel::Nvenc) => return Some("hevc_nvenc"),
        ("hevc" | "h265", HwAccel::Qsv) => return Some("hevc_qsv"),
        ("hevc" | "h265", HwAccel::Vaapi) => return Some("hevc_vaapi"),
        ("hevc" | "h265", HwAccel::VideoToolbox) => return Some("hevc_videotoolbox"),
        ("hevc" | "h265", HwAccel::Amf) => return Some("hevc_amf"),
        ("hevc" | "h265", HwAccel::None) => return Some("libx265"),
        (_, HwAccel::Nvenc) => ("h264_nvenc", ""),
        (_, HwAccel::Qsv) => ("h264_qsv", ""),
        (_, HwAccel::Vaapi) => ("h264_vaapi", ""),
        (_, HwAccel::VideoToolbox) => ("h264_videotoolbox", ""),
        (_, HwAccel::Amf) => ("h264_amf", ""),
        (_, HwAccel::None) => ("libx264", ""),
    };
    Some(if h264.is_empty() { hevc } else { h264 })
}

/// Dodatni argumenti za encoder (preset, rate control, pixel format).
pub fn encoder_args(hw: HwAccel, bitrate_kbps: u32, needs_software_pix_fmt: bool) -> Vec<String> {
    let bitrate = bitrate_kbps.max(500);
    let maxrate = (bitrate as f64 * 1.2) as u32;
    let bufsize = bitrate * 2;
    let rate = |args: &mut Vec<String>| {
        args.push("-b:v".to_string());
        args.push(format!("{bitrate}k"));
        args.push("-maxrate".to_string());
        args.push(format!("{maxrate}k"));
        args.push("-bufsize".to_string());
        args.push(format!("{bufsize}k"));
    };

    let mut args = Vec::new();
    match hw {
        HwAccel::Nvenc => {
            args.push("-preset".to_string());
            args.push("p4".to_string());
            args.push("-tune".to_string());
            args.push("ll".to_string());
            args.push("-rc".to_string());
            args.push("vbr".to_string());
            rate(&mut args);
            args.push("-spatial-aq".to_string());
            args.push("1".to_string());
        }
        HwAccel::Qsv => {
            args.push("-preset".to_string());
            args.push("medium".to_string());
            rate(&mut args);
        }
        HwAccel::Vaapi => {
            rate(&mut args);
        }
        HwAccel::VideoToolbox => {
            rate(&mut args);
            args.push("-allow_sw".to_string());
            args.push("1".to_string());
        }
        HwAccel::Amf => {
            rate(&mut args);
        }
        HwAccel::None => {
            args.push("-preset".to_string());
            args.push("veryfast".to_string());
            args.push("-tune".to_string());
            args.push("zerolatency".to_string());
            rate(&mut args);
            args.push("-pix_fmt".to_string());
            args.push("yuv420p".to_string());
        }
    }

    if needs_software_pix_fmt {
        args.push("-pix_fmt".to_string());
        args.push("yuv420p".to_string());
    }
    args
}

fn probe_binary(ffmpeg_path: &str) -> bool {
    Command::new(ffmpeg_path)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Testno enkodiranje 1 sekunde: `Ok` znaci da ubrzanje stvarno radi na ovom stroju.
fn test_encoder(ffmpeg_path: &str, hw: HwAccel) -> Result<(), String> {
    let mut args: Vec<String> =
        vec!["-hide_banner".to_string(), "-loglevel".to_string(), "error".to_string()];

    if hw == HwAccel::Vaapi {
        if !std::path::Path::new("/dev/dri/renderD128").exists() {
            return Err("nema /dev/dri/renderD128".to_string());
        }
        args.push("-vaapi_device".to_string());
        args.push("/dev/dri/renderD128".to_string());
    }

    args.extend([
        "-f".to_string(),
        "lavfi".to_string(),
        "-i".to_string(),
        "testsrc2=size=320x240:rate=5:duration=1".to_string(),
    ]);

    let encoder = match hw {
        HwAccel::Nvenc => "h264_nvenc",
        HwAccel::Qsv => "h264_qsv",
        HwAccel::Vaapi => "h264_vaapi",
        HwAccel::VideoToolbox => "h264_videotoolbox",
        HwAccel::Amf => "h264_amf",
        HwAccel::None => return Ok(()),
    };

    if hw == HwAccel::Vaapi {
        args.push("-vf".to_string());
        args.push("format=nv12,hwupload".to_string());
    }
    args.extend([
        "-c:v".to_string(),
        encoder.to_string(),
        "-f".to_string(),
        "null".to_string(),
        "-".to_string(),
    ]);

    let output = Command::new(ffmpeg_path)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| err.to_string())?;

    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let first = stderr.lines().find(|line| !line.trim().is_empty()).unwrap_or("nepoznata greska");
    Err(first.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_values() {
        assert_eq!(HwAccel::parse("auto"), None);
        assert_eq!(HwAccel::parse("nvenc"), Some(HwAccel::Nvenc));
        assert_eq!(HwAccel::parse("NVENC"), Some(HwAccel::Nvenc));
        assert_eq!(HwAccel::parse("videotoolbox"), Some(HwAccel::VideoToolbox));
        assert_eq!(HwAccel::parse("none"), Some(HwAccel::None));
        assert_eq!(HwAccel::parse("bogus"), None);
    }

    #[test]
    fn encoder_name_maps_back_to_family() {
        assert_eq!(HwAccel::from_encoder("h264_nvenc"), HwAccel::Nvenc);
        assert_eq!(HwAccel::from_encoder("hevc_vaapi"), HwAccel::Vaapi);
        assert_eq!(HwAccel::from_encoder("h264_videotoolbox"), HwAccel::VideoToolbox);
        assert_eq!(HwAccel::from_encoder("libx264"), HwAccel::None);
        assert_eq!(HwAccel::from_encoder("libx265"), HwAccel::None);
    }

    #[test]
    fn encoder_names_follow_codec_and_hw() {
        assert_eq!(video_encoder(HwAccel::Nvenc, "h264"), Some("h264_nvenc"));
        assert_eq!(video_encoder(HwAccel::Nvenc, "hevc"), Some("hevc_nvenc"));
        assert_eq!(video_encoder(HwAccel::Qsv, "hevc"), Some("hevc_qsv"));
        assert_eq!(video_encoder(HwAccel::None, "h264"), Some("libx264"));
        assert_eq!(video_encoder(HwAccel::None, "hevc"), Some("libx265"));
        assert_eq!(video_encoder(HwAccel::VideoToolbox, "h264"), Some("h264_videotoolbox"));
    }

    #[test]
    fn nvenc_args_carry_rate_control_and_low_latency() {
        let args = encoder_args(HwAccel::Nvenc, 8000, false);
        let joined = args.join(" ");
        assert!(joined.contains("-b:v 8000k"), "{joined}");
        assert!(joined.contains("-maxrate 9600k"), "{joined}");
        assert!(joined.contains("-bufsize 16000k"), "{joined}");
        assert!(joined.contains("-preset p4"), "{joined}");
        assert!(!joined.contains("pix_fmt"), "NVENC ne trazi yuv420p");
    }

    #[test]
    fn software_args_force_yuv420p_and_fast_preset() {
        let args = encoder_args(HwAccel::None, 4000, false);
        let joined = args.join(" ");
        assert!(joined.contains("-preset veryfast"), "{joined}");
        assert!(joined.contains("-pix_fmt yuv420p"), "{joined}");
    }

    #[test]
    fn bitrate_has_a_floor_to_avoid_broken_streams() {
        let args = encoder_args(HwAccel::None, 0, false);
        assert!(args.join(" ").contains("-b:v 500k"), "{args:?}");
    }

    #[test]
    fn unavailable_ffmpeg_degrades_to_software() {
        let support = detect("/nema/ffmpeg", "auto");
        assert_eq!(support.preferred, HwAccel::None);
        assert!(support.notes.iter().any(|note| note.contains("nije pokrenut")));
    }

    #[test]
    fn explicit_none_disables_hw() {
        let support = detect("ffmpeg", "none");
        assert_eq!(support.preferred, HwAccel::None);
        assert!(support.available.is_empty());
    }

    /// Stvarni ffmpeg na ovom stroju — samo provjera da detekcija ne paničari.
    #[test]
    fn real_detection_runs_without_panic() {
        let support = detect("ffmpeg", "auto");
        if support.available.is_empty() {
            eprintln!("nema HW ubrzanja na ovom stroju: {:?}", support.notes);
        } else {
            eprintln!("dostupno: {:?}, preferirano: {}", support.available, support.preferred.name());
        }
        assert!(support.supports(support.preferred));
    }
}
