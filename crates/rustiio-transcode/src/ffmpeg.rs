//! Gradnja i pokretanje ffmpeg naredbe.
//!
//! Sve sto ovdje nastaje je `Vec<String>` argumenata — pa se lako testira bez
//! pokretanja ijednog procesa. Pokretanje ([`spawn`]) je odvojeno od gradnje.

use std::path::Path;

use anyhow::Context;
use tokio::process::{Child, Command};

use crate::decision::{Decision, PlaybackMode};
use crate::hwaccel;

#[derive(Debug, Clone)]
pub struct StartRequest<'a> {
    pub input: &'a Path,
    /// Ekstenzija izvornog fajla (za `-bsf` kad idemo u MPEG-TS).
    pub source_ext: &'a str,
    pub decision: &'a Decision,
    pub ffmpeg_path: &'a str,
    /// Odakle poceti (seek u transcode streamu).
    pub start_at_ms: Option<u64>,
    /// Vanjski titl koji se upecava (samo ako `decision.burn_subtitles`).
    pub subtitle: Option<&'a Path>,
}

/// Argumenti za ffmpeg koji na stdout pise MPEG-TS (ili fragmented MP4).
pub fn build_args(request: &StartRequest<'_>) -> Vec<String> {
    let decision = request.decision;
    let mut args: Vec<String> = Vec::new();

    push(&mut args, &["-hide_banner", "-loglevel", "error", "-nostdin"]);

    if let Some(start_ms) = request.start_at_ms.filter(|value| *value > 0) {
        args.push("-ss".to_string());
        args.push(format!("{:.3}", start_ms as f64 / 1000.0));
    }

    // Hardversko dekodiranje: GPU raspakira sliku, pa procesor (ili GPU) enkodira.
    // Bez ovoga CPU radi i dekodiranje i enkodiranje — a to je pola posla.
    if decision.hardware_decode {
        if let Some(zastavice) = hwaccel::decode_args(decision.hw, decision.video_encoder.as_deref()) {
            args.extend(zastavice);
        }
    }

    args.push("-i".to_string());
    args.push(request.input.display().to_string());

    // Prvi video i prvi audio — ostalo TV ionako ne koristi.
    // Kod audio-only transcodea videa nema u izlazu.
    let audio_only = matches!(decision.mode, PlaybackMode::Transcode { video: false, audio: true });
    if !audio_only {
        push(&mut args, &["-map", "0:v:0"]);
    }
    push(&mut args, &["-map", "0:a:0?"]);

    match &decision.mode {
        PlaybackMode::Remux => {
            args.push("-c".to_string());
            args.push("copy".to_string());
            if decision.container == "mpegts" && needs_annexb(request.source_ext) {
                push(&mut args, &["-bsf:v", "h264_mp4toannexb"]);
            }
        }
        PlaybackMode::Transcode { .. } => {
            if let Some(encoder) = &decision.video_encoder {
                args.push("-c:v".to_string());
                args.push(encoder.clone());
                // Softverski enkoder: koliko niti smije uzeti (0 = sve jezgre).
                if decision.threads > 0 && hwaccel::HwAccel::from_encoder(encoder) == hwaccel::HwAccel::None {
                    push(&mut args, &["-threads", &decision.threads.to_string()]);
                }
                // Samsung (Serviio to izričito rješava u svom profilu): H.264 mora biti
                // High/Main **do levela 4.1**. Bez ovoga NVENC sam izabere npr. 5.1 i TV
                // prikaže krug bez slike — a ne prijavi grešku.
                if encoder.contains("h264") {
                    push(&mut args, &["-profile:v", "high", "-level", "4.1"]);
                }
                args.extend(hwaccel::encoder_args(
                    decision.hw,
                    decision.video_bitrate_kbps.unwrap_or(0),
                    decision.container == "mpegts" && !needs_annexb(request.source_ext),
                ));
                if let Some(filters) = video_filters(request) {
                    args.push("-vf".to_string());
                    args.push(filters);
                }
            } else {
                push(&mut args, &["-c:v", "copy"]);
            }

            if let Some(audio) = &decision.audio_encoder {
                args.push("-c:a".to_string());
                args.push(audio.clone());
                push(&mut args, &["-b:a", "192k"]);
                if let Some(channels) = decision.audio_channels {
                    args.push("-ac".to_string());
                    args.push(channels.to_string());
                }
            } else {
                push(&mut args, &["-c:a", "copy"]);
            }
        }
        PlaybackMode::Direct => {}
    }

    match decision.container.as_str() {
        "mp4" => push(&mut args, &["-movflags", "frag_keyframe+empty_moov+default_base_moof", "-f", "mp4"]),
        // `+resend_headers`: TV se zna priključiti u toku pa traži PAT/PMT ispočetka.
        _ => push(
            &mut args,
            &["-f", "mpegts", "-mpegts_flags", "+resend_headers", "-muxdelay", "0", "-muxpreload", "0"],
        ),
    }

    // MPEG-TS bez ovoga zna dati "non monotonically increasing dts" na seeku.
    push(&mut args, &["-fflags", "+genpts"]);
    args.push("pipe:1".to_string());
    args
}

/// Skaliranje i/ili upecavanje titla — jedan `-vf` lanac.
/// Velicina na koju se izvor svodi (cuva omjer, parne dimenzije, nikad ne povecava).
/// `None` znaci da ostaje kako je.
fn ciljna_velicina(
    izvor: Option<(u32, u32)>,
    max_sirina: Option<u32>,
    max_visina: Option<u32>,
) -> Option<(u32, u32)> {
    let (sirina, visina) = izvor?;
    if sirina == 0 || visina == 0 {
        return None;
    }
    let zeljena_sirina = max_sirina.unwrap_or(sirina) as f64;
    let zeljena_visina = max_visina.unwrap_or(visina) as f64;
    let faktor = (zeljena_sirina / f64::from(sirina)).min(zeljena_visina / f64::from(visina)).min(1.0);
    let parno = |vrijednost: f64| ((vrijednost as u32) / 2 * 2).max(2);
    let (nova_sirina, nova_visina) = (parno(f64::from(sirina) * faktor), parno(f64::from(visina) * faktor));
    if (nova_sirina, nova_visina) == (sirina, visina) { None } else { Some((nova_sirina, nova_visina)) }
}

fn video_filters(request: &StartRequest<'_>) -> Option<String> {
    let decision = request.decision;
    let mut filters: Vec<String> = Vec::new();

    // Reži po obje dimenzije iz profila: 2160x1080 uz profil 1920x1080 ostavlja
    // 2160 širine ako se gleda samo visina, a TV takav okvir odbije.
    let max_sirina = decision.max_width.filter(|value| *value > 0);
    let max_visina = decision.max_height.filter(|value| *value > 0);
    // H.264 level 4.1 (pinovan za Samsung) ne dopusta sliku vecu od 8192,
    // a MPEG-2 Main Level ne preko 1920
    // makrobloka: 1920x1080 = 8160 prolazi, 2160x1080 = 9180 ne. Bez ovoga
    // NVENC odbije posao, ffmpeg ne napise ni bajt, a TV vrti krug.
    let (max_sirina, max_visina) = if decision
        .video_encoder
        .as_deref()
        .is_some_and(|encoder| encoder.contains("h264") || encoder.contains("mpeg2video"))
    {
        (
            Some(max_sirina.map_or(1920, |sirina| sirina.min(1920))),
            Some(max_visina.map_or(1080, |visina| visina.min(1080))),
        )
    } else {
        (max_sirina, max_visina)
    };
    if let Some((sirina, visina)) = ciljna_velicina(decision.source_size, max_sirina, max_visina) {
        // Brojevi, ne izrazi: `scale='min(1920,iw)':...` ffmpeg odbije s
        // "Invalid argument" (EINVAL) — ne napise ni bajt, a TV vrti krug.
        filters.push(format!("scale={sirina}:{visina}"));
    }
    if decision.burn_subtitles {
        if let Some(subtitle) = request.subtitle {
            filters.push(format!("subtitles={}", escape_filter_path(subtitle)));
        }
    }
    if decision.hw == hwaccel::HwAccel::Vaapi {
        filters.push("format=nv12,hwupload".to_string());
    }

    // Izlazni enkoderi su 8-bitni (NVENC, QSV, VideoToolbox, libx264). 10-bit izvor
    // (`hevc Main 10`, `yuv420p10le`) inače obori enkoder s „10 bit encode not
    // supported": ffmpeg ne napiše ni bajt, a TV prijavi grešku. VAAPI već ima svoj
    // `format=nv12` (isto 8-bit).
    if decision.video_encoder.is_some() && !filters.iter().any(|filter| filter.contains("hwupload")) {
        filters.push("format=yuv420p".to_string());
    }

    if filters.is_empty() { None } else { Some(filters.join(",")) }
}

/// FFmpeg filter sintaksa trazi escapane `:`, `'` i `\` u putanjama.
fn escape_filter_path(path: &Path) -> String {
    let text = path.display().to_string();
    let escaped = text.replace('\\', "\\\\").replace(':', "\\:").replace('\'', "\\'");
    format!("filename='{escaped}'")
}

/// MP4/MOV drze H.264 u AVCC obliku; MPEG-TS trazi Annex B.
fn needs_annexb(ext: &str) -> bool {
    matches!(ext.trim().trim_start_matches('.').to_ascii_lowercase().as_str(), "mp4" | "m4v" | "mov" | "3gp")
}

fn push(args: &mut Vec<String>, values: &[&str]) {
    args.extend(values.iter().map(|value| value.to_string()));
}

/// Pokreni ffmpeg; stdout je stream koji ide pravo u HTTP tijelo.
pub fn spawn(request: &StartRequest<'_>) -> anyhow::Result<Child> {
    let args = build_args(request);
    tracing::debug!(ffmpeg = request.ffmpeg_path, args = %args.join(" "), "pokrecem ffmpeg");

    Command::new(request.ffmpeg_path)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("ne mogu pokrenuti {}", request.ffmpeg_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::Decision;
    use crate::hwaccel::{HwAccel, HwSupport};
    use std::path::PathBuf;

    fn decision(mode: PlaybackMode, container: &str, encoder: Option<&str>, hw: HwAccel) -> Decision {
        Decision {
            mode,
            reasons: Vec::new(),
            protocol_info: String::new(),
            mime: "video/mp2t".to_string(),
            container: container.to_string(),
            video_encoder: encoder.map(|value| value.to_string()),
            video_bitrate_kbps: Some(8000),
            source_size: Some((1920, 1080)),
            max_width: None,
            max_height: None,
            audio_encoder: Some("aac".to_string()),
            audio_channels: Some(2),
            burn_subtitles: false,
            hw,
            threads: 0,
            hardware_decode: false,
        }
    }

    fn request<'a>(
        decision: &'a Decision,
        source_ext: &'a str,
        input: &'a Path,
        subtitle: Option<&'a Path>,
    ) -> StartRequest<'a> {
        StartRequest { input, source_ext, decision, ffmpeg_path: "ffmpeg", start_at_ms: None, subtitle }
    }

    #[test]
    fn remux_copies_streams_into_mpegts() {
        let decision = decision(PlaybackMode::Remux, "mpegts", None, HwAccel::None);
        let input = PathBuf::from("/media/film.mkv");
        let args = build_args(&request(&decision, "mkv", &input, None));
        let joined = args.join(" ");

        assert!(joined.contains("-i /media/film.mkv"), "{joined}");
        assert!(joined.contains("-c copy"), "{joined}");
        assert!(joined.contains("-f mpegts"), "{joined}");
        assert!(joined.contains("-map 0:a:0?"), "{joined}");
        assert!(!joined.contains("-c:v h264"), "remux ne re-enkodira: {joined}");
        assert!(joined.ends_with("pipe:1"), "{joined}");
    }

    #[test]
    fn remux_from_mp4_to_ts_adds_annexb_bitstream_filter() {
        let decision = decision(PlaybackMode::Remux, "mpegts", None, HwAccel::None);
        let input = PathBuf::from("/media/film.mp4");
        let args = build_args(&request(&decision, "mp4", &input, None));
        assert!(args.join(" ").contains("-bsf:v h264_mp4toannexb"), "{args:?}");
    }

    #[test]
    fn h264_target_pins_profile_and_level_for_samsung() {
        // Serviio u Samsung profilu rješava upravo ovo: HIGH/MAIN > level 4.1 TV
        // ne pušta (vrti krug). Zato profil i level moraju biti zadani.
        let decision = decision(
            PlaybackMode::Transcode { video: true, audio: true },
            "mpegts",
            Some("h264_nvenc"),
            HwAccel::Nvenc,
        );
        let input = PathBuf::from("/media/lanterns.mkv");
        let joined = build_args(&request(&decision, "mkv", &input, None)).join(" ");
        assert!(joined.contains("-profile:v high"), "{joined}");
        assert!(joined.contains("-level 4.1"), "{joined}");
        assert!(joined.contains("-mpegts_flags +resend_headers"), "{joined}");
    }

    #[test]
    fn ten_bit_source_gets_eight_bit_output_and_width_cap() {
        // Lanterns (hevc Main 10, 2160x1080) je obarao h264_nvenc i TV je dobio 0
        // bajtova; uz to je ostajao preširok za profil (1920x1080).
        let mut decision = decision(
            PlaybackMode::Transcode { video: true, audio: true },
            "mpegts",
            Some("h264_nvenc"),
            HwAccel::Nvenc,
        );
        decision.source_size = Some((2160, 1080));
        decision.max_width = Some(1920);
        decision.max_height = Some(1080);
        let input = PathBuf::from("/media/lanterns.mkv");
        let joined = build_args(&request(&decision, "mkv", &input, None)).join(" ");

        assert!(joined.contains("format=yuv420p"), "10-bit ide u 8-bit: {joined}");
        // Brojevi, ne izrazi: 2160x1080 u okvir 1920x1080 daje 1920x960.
        assert!(joined.contains("scale=1920:960"), "skalirano na okvir: {joined}");
    }

    #[test]
    fn copy_does_not_force_pixel_format() {
        // Remux ne smije dobiti `format=`: to bi značilo re-enkodiranje.
        let decision = decision(PlaybackMode::Remux, "mpegts", None, HwAccel::None);
        let input = PathBuf::from("/media/film.mkv");
        let joined = build_args(&request(&decision, "mkv", &input, None)).join(" ");
        assert!(!joined.contains("format="), "remux kopira: {joined}");
    }

    #[test]
    fn nvenc_transcode_has_encoder_rate_control_and_aac_audio() {
        let decision = decision(
            PlaybackMode::Transcode { video: true, audio: true },
            "mpegts",
            Some("h264_nvenc"),
            HwAccel::Nvenc,
        );
        let input = PathBuf::from("/media/hevc.mkv");
        let joined = build_args(&request(&decision, "mkv", &input, None)).join(" ");

        assert!(joined.contains("-c:v h264_nvenc"), "{joined}");
        assert!(joined.contains("-preset p4"), "{joined}");
        assert!(joined.contains("-c:a aac"), "{joined}");
        assert!(joined.contains("-ac 2"), "{joined}");
        assert!(joined.contains("-f mpegts"), "{joined}");
    }

    #[test]
    fn seek_adds_input_flag_with_fractional_seconds() {
        let decision = decision(
            PlaybackMode::Transcode { video: true, audio: true },
            "mpegts",
            Some("libx264"),
            HwAccel::None,
        );
        let input = PathBuf::from("/media/film.mkv");
        let mut req = request(&decision, "mkv", &input, None);
        req.start_at_ms = Some(90_500);
        let joined = build_args(&req).join(" ");
        assert!(joined.contains("-ss 90.500"), "{joined}");
        // -ss mora biti PRIJE -i (brzi seek)
        assert!(joined.find("-ss").unwrap() < joined.find("-i ").unwrap());
    }

    #[test]
    fn burn_in_adds_subtitles_filter_before_frame() {
        let mut decision = decision(
            PlaybackMode::Transcode { video: true, audio: true },
            "mpegts",
            Some("libx264"),
            HwAccel::None,
        );
        decision.burn_subtitles = true;
        decision.max_height = Some(720);
        let input = PathBuf::from("/media/film.mkv");
        let subtitle = PathBuf::from("/media/Film (2026)/Film (2026).srt");
        let joined = build_args(&request(&decision, "mkv", &input, Some(&subtitle))).join(" ");
        assert!(joined.contains("scale=1280:720"), "{joined}");
        assert!(joined.contains("subtitles=filename='/media/Film (2026)/Film (2026).srt'"), "{joined}");
        assert!(joined.contains("-vf"), "{joined}");
    }

    #[test]
    fn mp4_output_uses_fragmented_flags() {
        let decision = decision(
            PlaybackMode::Transcode { video: true, audio: true },
            "mp4",
            Some("h264_videotoolbox"),
            HwAccel::VideoToolbox,
        );
        let input = PathBuf::from("/media/film.mkv");
        let joined = build_args(&request(&decision, "mkv", &input, None)).join(" ");
        assert!(joined.contains("-movflags frag_keyframe+empty_moov+default_base_moof"), "{joined}");
        assert!(joined.contains("-f mp4"), "{joined}");
        assert!(!joined.contains("mpegts"), "{joined}");
    }

    #[test]
    fn filters_escape_special_characters_in_paths() {
        let path = PathBuf::from("/media/a:b'd.mkv");
        assert_eq!(escape_filter_path(&path), "filename='/media/a\\:b\\'d.mkv'");
    }

    #[test]
    fn hw_support_is_only_used_for_transcode() {
        let support = HwSupport {
            available: vec![HwAccel::Nvenc],
            preferred: HwAccel::Nvenc,
            notes: Vec::new(),
            subtitles_filter: false,
            encoder: None,
            threads: 0,
            hardware_decode: true,
        };
        assert_eq!(hwaccel::video_encoder(support.preferred, "h264"), Some("h264_nvenc"));
    }

    #[test]
    fn software_encoder_gets_the_configured_thread_count() {
        let mut decision = decision(
            PlaybackMode::Transcode { video: true, audio: true },
            "mpegts",
            Some("libx264"),
            HwAccel::None,
        );
        decision.threads = 6;
        let input = PathBuf::from("/media/film.mkv");
        let joined = build_args(&request(&decision, "mkv", &input, None)).join(" ");
        assert!(joined.contains("-c:v libx264"), "{joined}");
        assert!(joined.contains("-threads 6"), "niti moraju stići do ffmpeg-a: {joined}");
    }

    #[test]
    fn hardware_encoder_never_gets_threads() {
        let mut decision = decision(
            PlaybackMode::Transcode { video: true, audio: true },
            "mpegts",
            Some("h264_nvenc"),
            HwAccel::Nvenc,
        );
        decision.threads = 6;
        let input = PathBuf::from("/media/film.mkv");
        let joined = build_args(&request(&decision, "mkv", &input, None)).join(" ");
        assert!(!joined.contains("-threads"), "NVENC ne prima -threads: {joined}");
    }

    #[test]
    fn hardware_decode_goes_before_the_input() {
        let mut decision = decision(
            PlaybackMode::Transcode { video: true, audio: true },
            "mpegts",
            Some("libx264"),
            HwAccel::Nvenc,
        );
        decision.hardware_decode = true;
        let input = PathBuf::from("/media/film.mkv");
        let joined = build_args(&request(&decision, "mkv", &input, None)).join(" ");
        assert!(joined.contains("-hwaccel cuda"), "nema hardverskog dekodiranja: {joined}");
        assert!(joined.find("-hwaccel").unwrap() < joined.find("-i ").unwrap(), "hwaccel ide prije ulaza");
    }

    #[test]
    fn decode_flags_are_absent_when_turned_off() {
        let decision = decision(
            PlaybackMode::Transcode { video: true, audio: true },
            "mpegts",
            Some("libx264"),
            HwAccel::Nvenc,
        );
        let input = PathBuf::from("/media/film.mkv");
        let joined = build_args(&request(&decision, "mkv", &input, None)).join(" ");
        assert!(!joined.contains("-hwaccel"), "HW dekodiranje je isključeno: {joined}");
    }
}
