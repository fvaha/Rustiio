//! Decision engine: direct play, remux ili transcode — i zasto.
//!
//! Pravilo: **sto manje posla**. Ako uredjaj moze original, saljemo original; ako mu
//! smeta samo kontejner, radimo remux (`-c copy`, jeftino); tek ako kodek ili
//! rezolucija ne idu, ukljucujemo pravi transcode.

use rustiio_library::MediaInfo;
use rustiio_profiles::Profile;
use rustiio_upnp::protocol::{self, ProtocolInfo};

use crate::hwaccel::{self, HwAccel, HwSupport};

/// Kako posluziti fajl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackMode {
    /// Originalni fajl, byte-range seek.
    Direct,
    /// Drugi kontejner, bez re-enkodiranja (`-c copy`).
    Remux,
    /// Ponovni encode: video i/ili zvuk.
    Transcode { video: bool, audio: bool },
}

impl PlaybackMode {
    pub fn is_direct(&self) -> bool {
        matches!(self, PlaybackMode::Direct)
    }

    pub fn label(&self) -> &'static str {
        match self {
            PlaybackMode::Direct => "direct play",
            PlaybackMode::Remux => "remux",
            PlaybackMode::Transcode { .. } => "transcode",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub mode: PlaybackMode,
    /// Zasto — ide u log i u UI (korisnik vidi da razumije odluku).
    pub reasons: Vec<String>,
    /// `protocolInfo` za DIDL `res` (gotov string).
    pub protocol_info: String,
    /// MIME za HTTP odgovor.
    pub mime: String,
    /// Kontejner izlaza (`mpegts`, `mp4`) ili izvorna ekstenzija za direct.
    pub container: String,
    /// Video encoder za ffmpeg (`h264_nvenc`, `libx264`, ...); `None` = kopiraj.
    pub video_encoder: Option<String>,
    pub video_bitrate_kbps: Option<u32>,
    /// Najveća širina koju profil dopušta (2160x1080 izvor mora se rezati i po
    /// širini, ne samo po visini).
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    /// Velicina izvora — skaliranje racunamo sami, bez izraza u filteru.
    pub source_size: Option<(u32, u32)>,
    /// Audio encoder; `None` = kopiraj.
    pub audio_encoder: Option<String>,
    pub audio_channels: Option<u8>,
    pub burn_subtitles: bool,
    pub hw: HwAccel,
    /// Niti za softverski enkoder (0 = pusti ffmpeg da odluči).
    pub threads: u32,
    /// Dekodiraj hardverski (ubrzava i kad enkodira procesor).
    pub hardware_decode: bool,
}

impl Decision {
    pub fn needs_ffmpeg(&self) -> bool {
        !self.mode.is_direct()
    }

    pub fn summary(&self) -> String {
        let mode = match &self.mode {
            PlaybackMode::Transcode { video, audio } => format!(
                "transcode ({}{})",
                if *video { "video" } else { "" },
                if *audio { if *video { "+audio" } else { "audio" } } else { "" }
            ),
            other => other.label().to_string(),
        };
        format!("{mode} -> {}", self.container)
    }
}

/// Odluci kako posluziti `media` za `profile`.
///
/// `extension` je ekstenzija fajla na disku (ono sto uredjaj mora demuksirati).
/// `has_subtitle` govori postoji li vanjski titl (za burn-in odluku).
pub fn decide(
    media: &MediaInfo,
    profile: &Profile,
    extension: &str,
    has_subtitle: bool,
    hw: &HwSupport,
) -> Decision {
    let ext = extension.trim().trim_start_matches('.').to_ascii_lowercase();
    let mut reasons = Vec::new();

    let video_ok = video_supported(media, profile, &mut reasons);
    let container_ok = profile.supports_container(&ext);
    let audio_ok = audio_supported(media, profile, &mut reasons);

    if !container_ok {
        reasons.push(format!(
            "kontejner .{ext} nije na popisu profila ({})",
            profile.video.containers.join(", ")
        ));
    }

    let mode = if video_ok && container_ok && audio_ok {
        reasons.push("sve sto uredjaj trazi je u fajlu".to_string());
        PlaybackMode::Direct
    } else if video_ok && audio_ok && !container_ok && profile.transcode.allow_remux {
        reasons.push("kodeci odgovaraju, samo kontejner mijenjamo (-c copy)".to_string());
        PlaybackMode::Remux
    } else {
        let video = !video_ok;
        // Nijemi film nema sto re-enkodirati (`audio_ok` je tada `true`).
        let mut audio = !audio_ok;

        // Serviio svoj Samsung profil rjesava tako da uz transcode videa uvijek ide i
        // AC-3 audio (tamo `targetACodec="ac3"`, 192 kb/s, 2 kanala). Samsung pusti
        // MPEG-TS s AC-3, ali ne s AAC-om ni DD+ u kontejneru koji mu sami miksamo.
        if video
            && !audio
            && media.audio.as_ref().is_some_and(|zvuk| !zvuk.codec.is_empty())
            && profile.transcode.container == "mpegts"
            && profile.transcode.audio_codec == "ac3"
        {
            audio = true;
            reasons.push("uz transcode videa ide AC-3 audio (MPEG-TS s AAC/DD+ TV ne pusti)".to_string());
        }

        if !video && !audio {
            // Nista se ne mora re-enkodirati — samo kontejner smeta. Profil koji ne
            // dopusta remux trazi puni re-encode (neki uredjaji ne vole `-c copy`).
            if profile.transcode.allow_remux {
                reasons.push("kodeci odgovaraju, samo kontejner mijenjamo (-c copy)".to_string());
                PlaybackMode::Remux
            } else {
                reasons.push("profil ne dopusta remux — re-enkodiram i video i audio".to_string());
                PlaybackMode::Transcode { video: true, audio: true }
            }
        } else {
            reasons.push(format!(
                "re-enkodiranje: {}{}",
                if video { "video" } else { "" },
                if audio { " audio" } else { "" }
            ));
            PlaybackMode::Transcode { video, audio }
        }
    };

    let burn_subtitles = has_subtitle
        && profile.subtitle_mode() == rustiio_profiles::SubtitleMode::Burn
        && hw.can_burn_subtitles();
    if has_subtitle && profile.subtitle_mode() == rustiio_profiles::SubtitleMode::Burn && !burn_subtitles {
        reasons.push(
            "burn-in nije moguc (ffmpeg bez 'subtitles' filtra) — titl ide kao zaseban resurs".to_string(),
        );
    }

    let mut odluka = match mode {
        PlaybackMode::Direct => direct_decision(profile, &ext, media, reasons, burn_subtitles, hw),
        PlaybackMode::Remux => remux_decision(profile, reasons, hw, media),
        PlaybackMode::Transcode { video, audio } => {
            transcode_decision(profile, reasons, burn_subtitles, hw, video, audio)
        }
    };
    odluka.source_size = media.video.as_ref().map(|video| (video.width, video.height));
    odluka
}

/// Remux: samo kontejner, `-c copy`. Nema PN-a (stream nije originalni fajl).
fn remux_decision(profile: &Profile, reasons: Vec<String>, hw: &HwSupport, media: &MediaInfo) -> Decision {
    let (mime, container) = output_container(profile);
    let mut info = ProtocolInfo::new(mime).with_op(&profile.dlna.op).with_flags(&profile.dlna.flags);
    // CI=1: sadrzaj je konverzija, ne original. Bez toga TV primijeni pravila za
    // original i stream mu „ne odgovara" (Samsung to prijavi kao gresku formata).
    info.ci = Some("1".to_string());
    Decision {
        mode: PlaybackMode::Remux,
        reasons,
        protocol_info: info.to_protocol_info(),
        mime: mime.to_string(),
        container: container.to_string(),
        video_encoder: None,
        video_bitrate_kbps: media.video.as_ref().and_then(|video| video.bitrate_kbps),
        source_size: None,
        max_width: None,
        max_height: None,
        audio_encoder: None,
        audio_channels: None,
        burn_subtitles: false,
        hw: hw.preferred,
        threads: 0,
        hardware_decode: hw.hardware_decode,
    }
}

/// Mapiranje izlaznog kontejnera u (MIME, ime za ffmpeg).
fn output_container(profile: &Profile) -> (&'static str, &'static str) {
    match profile.transcode.container.as_str() {
        "mp4" => ("video/mp4", "mp4"),
        // Uz `DLNA.ORG_PN=MPEG_TS_*` ide MIME `video/mpeg`. Samsung na `video/mp2t`
        // prijavi grešku formata (u informacijama onda piše "mp2t" i `.ts`).
        _ if profile.transcode.video_codec.eq_ignore_ascii_case("mpeg2video") => {
            ("video/mpeg", "mpegts")
        }
        _ => ("video/mp2t", "mpegts"),
    }
}

fn direct_decision(
    profile: &Profile,
    ext: &str,
    media: &MediaInfo,
    reasons: Vec<String>,
    burn_subtitles: bool,
    hw: &HwSupport,
) -> Decision {
    let info = protocol::guess_for_ext_with(ext, &profile.dlna.op, &profile.dlna.flags, profile.dlna.send_pn);
    Decision {
        mode: PlaybackMode::Direct,
        reasons,
        protocol_info: info.to_protocol_info(),
        mime: info.mime.clone(),
        container: ext.to_string(),
        video_encoder: None,
        video_bitrate_kbps: media.bitrate_kbps,
        source_size: None,
        max_width: None,
        max_height: None,
        audio_encoder: None,
        audio_channels: None,
        burn_subtitles,
        hw: hw.preferred,
        threads: 0,
        hardware_decode: hw.hardware_decode,
    }
}

fn transcode_decision(
    profile: &Profile,
    mut reasons: Vec<String>,
    burn_subtitles: bool,
    hw: &HwSupport,
    video: bool,
    audio: bool,
) -> Decision {
    let target = &profile.transcode;
    let (mime, container) = output_container(profile);

    // Za transcode NEMA DLNA.ORG_PN — profil je taj koji garantira da stream ide.
    // OP=00: live stream nema poznatu velicinu ni Range, pa se NE smije
    // oglasavati byte-seek (OP=01) — Samsung takav res odbije (samo HEAD, bez GET-a).
    let mut info = ProtocolInfo::new(mime).with_op("00").with_flags(&profile.dlna.flags);
    // CI=1: sadrzaj je konverzija, ne original. Bez toga TV primijeni pravila za
    // original i stream mu „ne odgovara" (Samsung to prijavi kao gresku formata).
    info.ci = Some("1".to_string());

    // Sta se ne re-enkodira, to se kopira (`-c:v copy` / `-c:a copy`).
    // Korisnikov izričit odabir enkodera ima prednost — ali samo ako je isti kodek
    // kao ono što profil traži (inače bi TV dobio kodek koji ne podržava).
    let izričit = hw.encoder.clone().filter(|name| {
        // Odabir iz sucelja (`h264_nvenc`) prihvaca se samo ako je to enkoder koji
        // ovaj kodek i ovo hardver stvarno daju. Inace bi za profil koji trazi
        // mpeg2video (Samsung) prosao h264_nvenc i TV bi dobio kodek koji ne pusta.
        hwaccel::video_encoder(HwAccel::from_encoder(name), &target.video_codec)
            .map(str::to_string)
            .as_deref()
            == Some(name.as_str())
    });
    if video && hw.encoder.is_some() && izričit.is_none() {
        reasons.push(format!(
            "odabrani enkoder {} ne odgovara kodeku {} — koristim preporučeni",
            hw.encoder.as_deref().unwrap_or(""),
            target.video_codec
        ));
    }
    let video_encoder = if video {
        izričit.or_else(|| hwaccel::video_encoder(hw.preferred, &target.video_codec).map(str::to_string))
    } else {
        None
    };
    // Kad je izabran HW enkoder, i pomoćne zastavice moraju biti njegove (npr. `-vaapi_device`).
    // Softverski enkoder ne smije dobiti hardverske zastavice (npr. `-preset p4`
    // za NVENC na `mpeg2video`) — ffmpeg bi pao na nepoznatoj opciji.
    let hw_za_ffmpeg = match video_encoder.as_deref() {
        Some(name) => HwAccel::from_encoder(name),
        None => hw.preferred,
    };
    let audio_encoder = if audio { Some(target.audio_codec.clone()) } else { None };

    // Samsung (Serviio `sam_j`) trazi MPEG-2 u MPEG-TS **s DLNA profilom**:
    // bez `DLNA.ORG_PN` televizor odbije stream iako su kodeci ispravni.
    let info = if container == "mpegts" && target.video_codec.eq_ignore_ascii_case("mpeg2video") {
        // Visina izvora nije poznata u ovoj funkciji; ciljna visina profila je dovoljna.
        let hd = target.max_height.unwrap_or(1080) > 576;
        info.with_pn(if hd { "MPEG_TS_HD_NA" } else { "MPEG_TS_SD_NA" })
    } else {
        info
    };

    Decision {
        mode: PlaybackMode::Transcode { video, audio },
        // Stvarnu velicinu izvora upisuje `decide` — skaliranje racuna iz nje.
        source_size: None,
        reasons,
        protocol_info: info.to_protocol_info(),
        mime: mime.to_string(),
        container: container.to_string(),
        video_encoder,
        video_bitrate_kbps: if video { Some(target.max_bitrate_kbps) } else { None },
        max_width: if video { Some(profile.video.max_width).filter(|sirina| *sirina > 0) } else { None },
        max_height: if video { target.max_height } else { None },
        audio_encoder,
        audio_channels: if audio { Some(target.audio_channels) } else { None },
        burn_subtitles: burn_subtitles && video,
        hw: hw_za_ffmpeg,
        threads: if video { hw.threads } else { 0 },
        hardware_decode: video && hw.hardware_decode,
    }
}

/// Kodek iz imena enkodera (`hevc_nvenc` → hevc, `libx264` → h264).

fn video_supported(media: &MediaInfo, profile: &Profile, reasons: &mut Vec<String>) -> bool {
    let Some(video) = &media.video else {
        return true; // zvucni fajl: nema videa koji bi smetao
    };
    let mut ok = true;

    if !profile.supports_video_codec(&video.codec) {
        reasons.push(format!("video kodek {} nije podrzan", video.codec));
        ok = false;
    }
    if !profile.fits_video(video.width, video.height) {
        reasons.push(format!(
            "rezolucija {}x{} prelazi profil ({}x{})",
            video.width, video.height, profile.video.max_width, profile.video.max_height
        ));
        ok = false;
    }
    if let (Some(bitrate), max) = (video.bitrate_kbps, profile.video.max_bitrate_kbps) {
        if max > 0 && bitrate > max {
            reasons.push(format!("bitrate {bitrate} kbps prelazi profil ({max} kbps)"));
            ok = false;
        }
    }
    // Dubina boje: HEVC Main 10 (`yuv420p10le`) ne otvara nijedan TV koji zna
    // samo 8-bit, a kodek je isti (`hevc`) pa ga sam popis kodeka ne uhvati.
    if let Some(dubina) = bit_depth(video)
        && !profile.supports_bit_depth(dubina)
    {
        reasons.push(format!(
            "{dubina}-bit video ({} {}) prelazi profil ({} bita)",
            video.codec,
            video.profile.as_deref().unwrap_or("-"),
            profile.video.max_bit_depth
        ));
        ok = false;
    }
    ok
}

/// Dubina boje iz ffprobe-a: iz `pix_fmt` (`yuv420p10le`) ili iz profila
/// (`Main 10`). Ništa se ne nagađa — kad nema podatka, ne smeta.
fn bit_depth(video: &rustiio_library::mediainfo::VideoStream) -> Option<u8> {
    video
        .pix_fmt
        .as_deref()
        .and_then(dubina_iz_teksta)
        .or_else(|| video.profile.as_deref().and_then(dubina_iz_teksta))
}

fn dubina_iz_teksta(tekst: &str) -> Option<u8> {
    let mala = tekst.to_ascii_lowercase();
    [16u8, 12, 10]
        .into_iter()
        .find(|dubina| mala.contains(&format!("p{dubina}")) || mala.contains(&format!(" {dubina}")))
}

fn audio_supported(media: &MediaInfo, profile: &Profile, reasons: &mut Vec<String>) -> bool {
    let Some(audio) = &media.audio else {
        return true; // nijemi film
    };
    if !profile.supports_audio_codec(&audio.codec) {
        reasons.push(format!("audio kodek {} nije podrzan", audio.codec));
        return false;
    }
    if audio.channels > profile.audio.max_channels {
        reasons.push(format!(
            "{} kanala prelazi profil ({} kanala)",
            audio.channels, profile.audio.max_channels
        ));
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hwaccel::HwSupport;
    use rustiio_library::{AudioStream, VideoStream};

    fn hw_soft() -> HwSupport {
        HwSupport {
            available: Vec::new(),
            preferred: HwAccel::None,
            notes: Vec::new(),
            subtitles_filter: false,
            encoder: None,
            threads: 0,
            hardware_decode: false,
        }
    }

    fn hw_nvenc() -> HwSupport {
        HwSupport {
            available: vec![HwAccel::Nvenc],
            preferred: HwAccel::Nvenc,
            notes: Vec::new(),
            subtitles_filter: false,
            encoder: None,
            threads: 0,
            hardware_decode: true,
        }
    }

    fn media(video_codec: &str, width: u32, height: u32, audio_codec: &str, channels: u8) -> MediaInfo {
        MediaInfo {
            container: "matroska,webm".to_string(),
            duration_ms: Some(5_400_000),
            bitrate_kbps: Some(8000),
            size_bytes: 5_000_000_000,
            video: Some(VideoStream {
                codec: video_codec.to_string(),
                width,
                height,
                bitrate_kbps: Some(8000),
                pix_fmt: Some("yuv420p".to_string()),
                profile: None,
                level: None,
            }),
            audio: Some(AudioStream {
                index: 1,
                codec: audio_codec.to_string(),
                channels,
                language: None,
                bitrate_kbps: Some(448),
            }),
            audio_streams: Vec::new(),
            embedded_subtitles: 0,
            subtitles: Vec::new(),
        }
    }

    /// Isti zapis, ali 10-bit (HEVC Main 10 / yuv420p10le).
    fn media_10bit(video_codec: &str, width: u32, height: u32, audio_codec: &str, channels: u8) -> MediaInfo {
        let mut info = media(video_codec, width, height, audio_codec, channels);
        if let Some(video) = info.video.as_mut() {
            video.pix_fmt = Some("yuv420p10le".to_string());
            video.profile = Some("Main 10".to_string());
        }
        info
    }

    #[test]
    fn ten_bit_video_is_transcoded_for_an_eight_bit_tv() {
        let deset = media_10bit("hevc", 1920, 1080, "eac3", 6);
        let decision = decide(&deset, &profile("samsung-tv"), "mkv", false, &hw_soft());
        assert_eq!(
            decision.mode,
            PlaybackMode::Transcode { video: true, audio: true },
            "10-bit HEVC se mora prekodirati (uz AC-3 audio): {:?}",
            decision.reasons
        );
        assert_eq!(decision.audio_encoder.as_deref(), Some("ac3"));
        assert!(
            decision.reasons.iter().any(|reason| reason.contains("10-bit")),
            "razlog mora reci da je 10-bit: {:?}",
            decision.reasons
        );
        // Isti kodek u 8-bit ide direktno — ne dira se sve, samo ono što TV ne može.
        let osam = media("hevc", 1920, 1080, "eac3", 6);
        assert_eq!(
            decide(&osam, &profile("samsung-tv"), "mkv", false, &hw_soft()).mode,
            PlaybackMode::Direct
        );
    }

    #[test]
    fn desktop_clients_take_ten_bit_directly() {
        let deset = media_10bit("hevc", 3840, 2160, "eac3", 6);
        assert_eq!(
            decide(&deset, &profile("vlc"), "mkv", false, &hw_soft()).mode,
            PlaybackMode::Direct,
            "VLC profil dopušta 10-bit"
        );
    }

    fn profile(id: &str) -> Profile {
        rustiio_profiles::builtin::load().get(id).unwrap().clone()
    }

    #[test]
    fn samsung_plays_h264_mkv_directly() {
        let decision =
            decide(&media("h264", 1920, 1080, "ac3", 6), &profile("samsung-tv"), "mkv", false, &hw_soft());
        assert_eq!(decision.mode, PlaybackMode::Direct);
        assert!(!decision.needs_ffmpeg());
        assert!(decision.protocol_info.contains("video/x-matroska"));
        assert!(decision.reasons.iter().any(|reason| reason.contains("sve sto uredjaj trazi")));
    }

    #[test]
    fn hevc_on_generic_tv_needs_transcode_with_nvenc_when_available() {
        let decision =
            decide(&media("hevc", 1920, 1080, "eac3", 6), &profile("generic"), "mkv", false, &hw_nvenc());
        assert_eq!(decision.mode, PlaybackMode::Transcode { video: true, audio: true });
        assert_eq!(decision.video_encoder.as_deref(), Some("h264_nvenc"));
        assert_eq!(decision.container, "mpegts");
        assert_eq!(decision.mime, "video/mp2t");
        assert!(!decision.protocol_info.contains("DLNA.ORG_PN"), "transcode stream nema PN");
        assert!(decision.reasons.iter().any(|reason| reason.contains("hevc")));
        assert!(decision.reasons.iter().any(|reason| reason.contains("eac3")));
    }

    #[test]
    fn software_encoder_is_used_when_no_hw() {
        let decision =
            decide(&media("hevc", 1920, 1080, "eac3", 6), &profile("generic"), "mkv", false, &hw_soft());
        assert_eq!(decision.video_encoder.as_deref(), Some("libx264"));
    }

    #[test]
    fn unsupported_container_with_good_codecs_is_remuxed() {
        // Kodi pusta sve, ali RealMedia kontejner nije na popisu.
        let decision = decide(&media("h264", 1280, 720, "ac3", 6), &profile("kodi"), "rm", false, &hw_soft());
        assert_eq!(decision.mode, PlaybackMode::Remux);
        assert!(decision.video_encoder.is_none(), "remux ne re-enkodira");
        assert_eq!(decision.container, "mpegts");
    }

    #[test]
    fn too_high_bitrate_forces_transcode() {
        let mut heavy = media("h264", 1920, 1080, "ac3", 6);
        heavy.video.as_mut().unwrap().bitrate_kbps = Some(90_000);
        let mut limited = profile("generic");
        limited.video.max_bitrate_kbps = 20_000;
        let decision = decide(&heavy, &limited, "mkv", false, &hw_soft());
        assert!(matches!(decision.mode, PlaybackMode::Transcode { video: true, .. }));
        assert!(decision.reasons.iter().any(|reason| reason.contains("bitrate")));
    }

    #[test]
    fn container_only_mismatch_without_remux_is_full_transcode() {
        // Profil koji ne dopusta remux: kad smeta samo kontejner, ide puni re-encode —
        // nikad "transcode bez icega" (video=false, audio=false).
        let mut profile = profile("generic");
        profile.transcode.allow_remux = false;
        let decision = decide(&media("h264", 1280, 720, "aac", 2), &profile, "rm", false, &hw_soft());
        assert_eq!(decision.mode, PlaybackMode::Transcode { video: true, audio: true });
        assert!(decision.video_encoder.is_some());
        assert!(decision.audio_encoder.is_some());
    }

    #[test]
    fn container_only_mismatch_with_remux_copies_both_streams() {
        let decision = decide(&media("h264", 1280, 720, "aac", 2), &profile("kodi"), "rm", false, &hw_soft());
        assert_eq!(decision.mode, PlaybackMode::Remux);
        assert!(decision.video_encoder.is_none());
        assert!(decision.audio_encoder.is_none());
    }

    #[test]
    fn burn_in_needs_ffmpeg_subtitles_filter() {
        let subtitles = rustiio_profiles::SubtitleCaps {
            mode: rustiio_profiles::SubtitleMode::Burn,
            ..rustiio_profiles::SubtitleCaps::default()
        };
        let profile = Profile { subtitles, ..profile("generic") };
        let media = media("hevc", 1280, 720, "aac", 2);

        // Bez filtra: titl ostaje soft (bolje ista slika nego prazan stream).
        let without = decide(&media, &profile, "mkv", true, &hw_soft());
        assert!(!without.burn_subtitles, "bez filtra se ne upecava");
        assert!(without.reasons.iter().any(|reason| reason.contains("burn-in nije moguc")));

        // S filtrom: upecava se.
        let mut hw = HwSupport::software();
        hw.subtitles_filter = true;
        let with = decide(&media, &profile, "mkv", true, &hw);
        assert!(with.burn_subtitles);
    }

    #[test]
    fn silent_video_is_not_audio_re_encoded() {
        let mut media = media("hevc", 1920, 1080, "aac", 2);
        media.audio = None;
        media.audio_streams.clear();
        // Generic profil ne zna HEVC → video se re-enkodira, audio ne (nema ga).
        let decision = decide(&media, &profile("generic"), "mkv", false, &hw_soft());
        assert_eq!(decision.mode, PlaybackMode::Transcode { video: true, audio: false });
        assert!(decision.audio_encoder.is_none());
    }

    #[test]
    fn video_transcode_also_makes_ac3_audio() {
        // Samsung pusta TS s AC-3; eac3 (DD+) mu je u tom kontejneru problem iako ga
        // profil inace podrzava, pa uz transcode videa ide i AC-3.
        let deset = media_10bit("hevc", 1920, 1080, "eac3", 6);
        let decision = decide(&deset, &profile("samsung-tv"), "mkv", true, &hw_soft());
        assert!(matches!(decision.mode, PlaybackMode::Transcode { video: true, audio: true }));
        assert_eq!(decision.audio_encoder.as_deref(), Some("ac3"));
        assert_eq!(decision.audio_channels, Some(2));
    }

    #[test]
    fn audio_only_flac_on_limited_tv_gets_audio_transcode() {
        let mut music = media("", 0, 0, "flac", 2);
        music.video = None;
        music.audio_streams = music.audio.clone().into_iter().collect();
        let decision = decide(&music, &profile("generic"), "flac", false, &hw_soft());
        assert!(matches!(decision.mode, PlaybackMode::Transcode { video: false, audio: true }));
        // Ciljni audio dolazi iz profila: Samsung traži AC-3 u TS-u (Serviio isto).
        assert_eq!(decision.audio_encoder.as_deref(), Some("ac3"));
        assert!(decision.video_encoder.is_none() || decision.video_encoder.as_deref() == Some("libx264"));
    }

    #[test]
    fn old_samsung_burns_subtitles() {
        // Burn-in putanja: profil to trazi, a ffmpeg to i moze (ima `subtitles` filter).
        let mut hw = hw_soft();
        hw.subtitles_filter = true;
        let decision = decide(&media("h264", 1280, 720, "ac3", 2), &profile("samsung-old"), "mkv", true, &hw);
        assert!(decision.burn_subtitles, "stari Samsung ne cita srt kao resurs");
    }

    #[test]
    fn modern_samsung_keeps_subtitles_soft() {
        let decision =
            decide(&media("h264", 1920, 1080, "ac3", 6), &profile("samsung-tv"), "mkv", true, &hw_soft());
        assert!(!decision.burn_subtitles);
        assert_eq!(decision.mode, PlaybackMode::Direct);
    }

    #[test]
    fn xbox_profile_drops_dlna_pn() {
        let decision =
            decide(&media("h264", 1920, 1080, "aac", 2), &profile("xbox"), "mp4", false, &hw_soft());
        assert_eq!(decision.mode, PlaybackMode::Direct);
        assert!(!decision.protocol_info.contains("DLNA.ORG_PN"), "Xbox profil ne salje PN");
        assert!(decision.protocol_info.contains("video/mp4"));
    }

    #[test]
    fn chosen_encoder_is_used_when_it_matches_the_codec() {
        let mut hw = hw_soft();
        hw.encoder = Some("libx264".to_string());
        hw.threads = 6;
        // mpeg2 na generičkom TV-u mora u transcode, a profil traži h264.
        let decision = decide(&media("mpeg2", 1920, 1080, "ac3", 6), &profile("generic"), "mkv", false, &hw);
        assert_eq!(decision.video_encoder.as_deref(), Some("libx264"));
        assert_eq!(decision.threads, 6, "niti iz configa idu u odluku");
    }

    #[test]
    fn mismatched_encoder_is_refused_with_a_reason() {
        // Profil traži h264, a korisnik je izabrao hevc enkoder → ne smije ga uzeti.
        let mut hw = hw_soft();
        hw.encoder = Some("libx265".to_string());
        let decision = decide(&media("mpeg2", 1920, 1080, "ac3", 6), &profile("generic"), "mkv", false, &hw);
        let encoder = decision.video_encoder.as_deref().expect("mora nešto enkodirati");
        assert!(encoder.contains("264"), "kodek mora ostati h264: {encoder}");
        assert!(
            decision.reasons.iter().any(|reason| reason.contains("ne odgovara kodeku")),
            "korisnik mora vidjeti zašto: {:?}",
            decision.reasons
        );
    }

    #[test]
    fn chosen_hardware_encoder_sets_the_hw_family_for_ffmpeg() {
        let mut hw = hw_nvenc();
        hw.encoder = Some("h264_nvenc".to_string());
        let decision = decide(&media("mpeg2", 1920, 1080, "ac3", 6), &profile("generic"), "mkv", false, &hw);
        assert_eq!(decision.video_encoder.as_deref(), Some("h264_nvenc"));
        assert_eq!(decision.hw, HwAccel::Nvenc, "ffmpeg zastavice idu po izabranom enkoderu");
        assert!(decision.hardware_decode);
    }
}
