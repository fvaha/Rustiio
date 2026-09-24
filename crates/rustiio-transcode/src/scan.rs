//! Sken sustava za transcode: sto je na stroju i cime je najbrze.
//!
//! Zašto mjerenje, a ne popis: `ffmpeg -encoders` ispiše i enkodere kojih GPU nema,
//! a brzina se ne vidi iz zastavica. Zato svaki enkoder koji je u popisu dobije
//! kratki test (2 s izvora, 1280x720) i mjeri se stvarni `fps`. Preporuka se onda
//! ne temelji na pretpostavci nego na broju.
//!
//! Sektori: traženje alata, popis hardvera (CPU/GPU), mjerenje enkodera, preporuka.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::hwaccel::HwAccel;

/// Jedan nađeni ffmpeg/ffprobe.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCandidate {
    pub path: String,
    /// Odakle je: `priložen uz program`, `postavka`, `PATH`, `uobičajeno mjesto`.
    pub source: String,
    pub version: Option<String>,
    pub works: bool,
    /// Priložen uz naš binarni fajl (najsigurniji — ne ovisi o sustavu).
    pub bundled: bool,
}

/// Procesor i koliko se niti smije koristiti.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CpuInfo {
    pub model: String,
    pub cores: usize,
    pub threads: usize,
}

/// Grafička kartica / uređaj za ubrzanje.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuInfo {
    pub name: String,
    pub kind: HwAccel,
    pub memory_mb: Option<u32>,
    pub note: String,
}

/// Izmjereni enkoder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EncoderProbe {
    /// Ime u ffmpeg-u, npr. `h264_nvenc`.
    pub encoder: String,
    pub hw: HwAccel,
    pub works: bool,
    /// Izmjereno: koliko sličica u sekundi (1280x720).
    pub fps: Option<f64>,
    pub note: String,
}

/// Preporuka: što upisati u config da transcode radi najbolje što stroj dopušta.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Recommendation {
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
    /// `gpu`, `cpu`, `hybrid` ili `auto`.
    pub mode: String,
    pub encoder: String,
    /// Niti za softverski enkoder (0 = ne diraj).
    pub threads: u32,
    pub hardware_decode: bool,
    pub max_concurrent: u32,
    /// Rečenica na hrvatskom (log i dijagnostika); UI slaže svoju iz brojki ispod.
    pub reason: String,
    /// Izmjerena brzina odabranog enkodera (sličica/s).
    pub best_fps: Option<f64>,
    /// Brzina s kojom se odabrani uspoređivao (drugi najbolji, sličica/s).
    pub other_fps: Option<f64>,
}

/// Sve što sken nađe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub ffmpeg: Vec<ToolCandidate>,
    pub ffprobe: Vec<ToolCandidate>,
    pub cpu: CpuInfo,
    pub gpus: Vec<GpuInfo>,
    pub encoders: Vec<EncoderProbe>,
    pub subtitles_filter: bool,
    pub recommendation: Recommendation,
}

/// Skeniraj stroj. `configured_*` su putanje iz configa (mogu biti prazne).
pub fn scan(configured_ffmpeg: &str, configured_ffprobe: &str, mode: &str) -> ScanReport {
    let ffmpeg = tool_candidates("ffmpeg", configured_ffmpeg);
    let ffprobe = tool_candidates("ffprobe", configured_ffprobe);
    let cpu = cpu_info();
    let best_ffmpeg = ffmpeg
        .iter()
        .find(|tool| tool.works)
        .map(|tool| tool.path.clone())
        .unwrap_or_else(|| "ffmpeg".to_string());

    let gpus = gpu_info(&best_ffmpeg);
    let (subtitles_filter, present) = encoder_list(&best_ffmpeg);
    let encoders = measure_encoders(&best_ffmpeg, &present);
    let recommendation = recommend(&ffmpeg, &ffprobe, &cpu, &encoders, mode);

    ScanReport { ffmpeg, ffprobe, cpu, gpus, encoders, subtitles_filter, recommendation }
}

/// Kandidati za alat: priložen uz program → postavka → PATH → uobičajena mjesta.
pub fn tool_candidates(name: &str, configured: &str) -> Vec<ToolCandidate> {
    let mut out: Vec<ToolCandidate> = Vec::new();
    let mut dodaj = |path: PathBuf, source: &str, bundled: bool| {
        // PATH ima desetke mapa bez ffmpeg-a (na macOS-u 30+) — u popis ide samo
        // ono što stvarno postoji, plus priloženi/postavljeni kandidat (da se vidi i kad ga nema).
        if !path.exists() && !bundled && source != "postavka" {
            return;
        }
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if out.iter().any(|tool| {
            std::fs::canonicalize(&tool.path).unwrap_or_else(|_| PathBuf::from(&tool.path)) == key
        }) {
            return;
        }
        let works = probe_binary(&path);
        let version = if works { binary_version(&path) } else { None };
        out.push(ToolCandidate {
            path: path.display().to_string(),
            source: source.to_string(),
            version,
            works,
            bundled,
        });
    };

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dodaj(dir.join(name), "priložen uz program", true);
        }
    }
    if !configured.trim().is_empty() {
        dodaj(PathBuf::from(configured), "postavka", false);
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            dodaj(dir.join(name), "PATH", false);
        }
    }
    for dir in ["/usr/local/bin", "/usr/bin", "/opt/homebrew/bin", "/snap/bin"] {
        dodaj(Path::new(dir).join(name), "uobičajeno mjesto", false);
    }
    out
}

fn probe_binary(path: &Path) -> bool {
    Command::new(path)
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn binary_version(path: &Path) -> Option<String> {
    let output = Command::new(path).arg("-version").stdin(Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().next().map(|line| line.trim().to_string())
}

pub fn cpu_info() -> CpuInfo {
    let threads = std::thread::available_parallelism().map(|value| value.get()).unwrap_or(1);
    let model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("model name") || line.starts_with("Model"))
                .and_then(|line| line.split(':').nth(1))
                .map(|value| value.trim().to_string())
        })
        .or_else(|| {
            // macOS: brand string preko sysctl-a.
            Command::new("sysctl")
                .args(["-n", "machdep.cpu.brand_string"])
                .output()
                .ok()
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "nepoznat procesor".to_string());
    CpuInfo { model, cores: threads, threads }
}

/// Grafičke kartice koje se mogu iskoristiti (NVIDIA upit, VAAPI čvorovi, Apple SoC).
pub fn gpu_info(ffmpeg_path: &str) -> Vec<GpuInfo> {
    let mut out = Vec::new();
    if let Ok(output) = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader"])
        .stdin(Stdio::null())
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let mut dijelovi = line.split(',').map(|value| value.trim());
                let name = dijelovi.next().unwrap_or("NVIDIA GPU").to_string();
                let memory = dijelovi
                    .next()
                    .and_then(|value| value.split_whitespace().next())
                    .and_then(|value| value.parse::<u32>().ok());
                out.push(GpuInfo {
                    name,
                    kind: HwAccel::Nvenc,
                    memory_mb: memory,
                    note: "NVENC/NVDEC".to_string(),
                });
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir("/dev/dri") {
        let mut cvorovi: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with("renderD"))
            .collect();
        cvorovi.sort();
        for cvor in cvorovi {
            out.push(GpuInfo {
                name: format!("VAAPI ({cvor})"),
                kind: HwAccel::Vaapi,
                memory_mb: None,
                note: format!("/dev/dri/{cvor}"),
            });
        }
    }
    // Apple: ako VideoToolbox enkoder radi, ubrzanje postoji (nema što drugo pitati).
    if cfg!(target_os = "macos")
        && probe_encoder(ffmpeg_path, "h264_videotoolbox", HwAccel::VideoToolbox).works
    {
        out.push(GpuInfo {
            name: "Apple VideoToolbox (SoC GPU)".to_string(),
            kind: HwAccel::VideoToolbox,
            memory_mb: None,
            note: "ujedinjena memorija".to_string(),
        });
    }
    out
}

/// Popis enkodera iz `ffmpeg -encoders` + ima li `subtitles` filter.
pub fn encoder_list(ffmpeg_path: &str) -> (bool, Vec<String>) {
    let output = Command::new(ffmpeg_path).args(["-hide_banner", "-encoders"]).stdin(Stdio::null()).output();
    let mut present = Vec::new();
    if let Ok(output) = output {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            for kandidat in KANDIDATI {
                if line.split_whitespace().any(|token| token == kandidat)
                    && !present.contains(&kandidat.to_string())
                {
                    present.push(kandidat.to_string());
                }
            }
        }
    }
    (has_subtitles_filter(ffmpeg_path), present)
}

/// Enkoderi koje probamo, redom od najbržeg prema najkompatibilnijem.
pub const KANDIDATI: [&str; 9] = [
    "h264_nvenc",
    "hevc_nvenc",
    "h264_qsv",
    "h264_vaapi",
    "h264_videotoolbox",
    "h264_amf",
    "libx264",
    "libx265",
    "hevc_vaapi",
];

fn has_subtitles_filter(ffmpeg_path: &str) -> bool {
    Command::new(ffmpeg_path)
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

fn measure_encoders(ffmpeg_path: &str, present: &[String]) -> Vec<EncoderProbe> {
    present
        .iter()
        .map(|encoder| probe_encoder(ffmpeg_path, encoder, HwAccel::from_encoder(encoder)))
        .collect()
}

/// Testno enkodiranje 2 s izvora u 720p; `fps` iz ispisa je mjera brzine.
pub fn probe_encoder(ffmpeg_path: &str, encoder: &str, hw: HwAccel) -> EncoderProbe {
    let mut args: Vec<String> =
        vec!["-hide_banner".to_string(), "-nostdin".to_string(), "-f".to_string(), "lavfi".to_string()];
    if hw == HwAccel::Vaapi {
        if !Path::new("/dev/dri/renderD128").exists() {
            return EncoderProbe {
                encoder: encoder.to_string(),
                hw,
                works: false,
                fps: None,
                note: "nema /dev/dri/renderD128".to_string(),
            };
        }
        args.extend(["-vaapi_device".to_string(), "/dev/dri/renderD128".to_string()]);
    }
    args.extend(["-i".to_string(), "testsrc2=size=1280x720:rate=30:duration=2".to_string()]);
    if hw == HwAccel::Vaapi {
        args.extend(["-vf".to_string(), "format=nv12,hwupload".to_string()]);
    }
    args.extend([
        "-c:v".to_string(),
        encoder.to_string(),
        "-f".to_string(),
        "null".to_string(),
        "-".to_string(),
    ]);

    let pocetak = std::time::Instant::now();
    match Command::new(ffmpeg_path).args(&args).stdin(Stdio::null()).output() {
        Ok(output) if output.status.success() => {
            // Mjeri se **stvarno trajanje** posla (2 s izvora = 60 sličica), a ne
            // ffmpegov `fps=` iz napretka — on na brzim enkoderima ispiše 0 ili
            // preskoči liniju, pa je NVENC izgledao sporiji od procesora.
            let sekundi = pocetak.elapsed().as_secs_f64().max(0.001);
            let iz_vremena = 60.0 / sekundi;
            let fps = match parse_fps(&String::from_utf8_lossy(&output.stderr)) {
                Some(value) if value > 1.0 => value.max(iz_vremena),
                _ => iz_vremena,
            };
            EncoderProbe {
                encoder: encoder.to_string(),
                hw,
                works: true,
                fps: Some(fps),
                note: format!("{fps:.0} sličica/s (720p, izmjereno)"),
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let first = stderr.lines().find(|line| !line.trim().is_empty()).unwrap_or("ne radi");
            EncoderProbe {
                encoder: encoder.to_string(),
                hw,
                works: false,
                fps: None,
                note: first.trim().chars().take(140).collect(),
            }
        }
        Err(error) => EncoderProbe {
            encoder: encoder.to_string(),
            hw,
            works: false,
            fps: None,
            note: error.to_string(),
        },
    }
}

/// Iz ffmpeg ispisa izvuci zadnji `fps=…` (zadnja linija je najtočnija).
pub fn parse_fps(stderr: &str) -> Option<f64> {
    let mut zadnji = None;
    // ffmpeg napredak piše s `\r`, a vrijednost zna biti i odvojeno (`fps= 145`).
    let mut prethodni_bio_fps = false;
    for token in stderr.split(|znak: char| znak == '\r' || znak == '\n' || znak.is_whitespace()) {
        if prethodni_bio_fps {
            if let Ok(number) = token.trim().parse::<f64>() {
                zadnji = Some(number);
            }
            prethodni_bio_fps = false;
            continue;
        }
        if let Some(value) = token.strip_prefix("fps=") {
            if value.trim().is_empty() {
                prethodni_bio_fps = true;
            } else if let Ok(number) = value.trim().parse::<f64>() {
                zadnji = Some(number);
            }
        }
    }
    zadnji
}

/// Najbrži enkoder (hardverski ili softverski), ali **h264 kad je dovoljno brz**.
///
/// Zašto: gotovo svaki TV pušta h264, a hevc je na istom čipu obično 10-20 % brži.
/// Kad je h264 iznad praga (60 sličica/s = dvije 1080p struje), ta razlika se ne vidi
/// na TV-u, a kompatibilnost se vidi. Ispod praga se uzima najbrži (i hevc ako treba).
fn najbolji<'a>(radi: &[&'a EncoderProbe], hardver: bool) -> Option<&'a EncoderProbe> {
    let grupa: Vec<&EncoderProbe> =
        radi.iter().copied().filter(|probe| (probe.hw != HwAccel::None) == hardver).collect();
    let brzina = |probe: &&EncoderProbe| probe.fps.unwrap_or(0.0);
    let najbrzi = grupa
        .iter()
        .copied()
        .max_by(|a, b| brzina(a).partial_cmp(&brzina(b)).unwrap_or(std::cmp::Ordering::Equal))?;
    let h264_klasa = grupa
        .iter()
        .copied()
        .filter(|probe| !probe.encoder.contains("265") && !probe.encoder.contains("hevc"))
        .max_by(|a, b| brzina(a).partial_cmp(&brzina(b)).unwrap_or(std::cmp::Ordering::Equal));
    // Prag: jedna 1080p struja traži ~30 sličica/s, dvije 60 — sve iznad toga je
    // rezerva koja se ne vidi, a hevc plaća kompatibilnošću na TV-ima.
    const PRAG: f64 = 60.0;
    match h264_klasa {
        Some(h264) if brzina(&h264) >= PRAG => Some(h264),
        _ => Some(najbrzi),
    }
}

/// Izbor: najbrži enkoder koji stvarno radi, uz korekciju prema traženom načinu.
pub fn recommend(
    ffmpeg: &[ToolCandidate],
    ffprobe: &[ToolCandidate],
    cpu: &CpuInfo,
    encoders: &[EncoderProbe],
    mode: &str,
) -> Recommendation {
    let ffmpeg_path = ffmpeg
        .iter()
        .find(|tool| tool.works)
        .map(|tool| tool.path.clone())
        .unwrap_or_else(|| "ffmpeg".to_string());
    let ffprobe_path = ffprobe
        .iter()
        .find(|tool| tool.works)
        .map(|tool| tool.path.clone())
        .unwrap_or_else(|| "ffprobe".to_string());

    let radi: Vec<&EncoderProbe> = encoders.iter().filter(|probe| probe.works).collect();
    // Niti za procesor: ostavi četvrtinu stroja serveru (SSDP, UI, baza).
    let niti = |threads: usize| -> u32 { ((threads.saturating_mul(3)) / 4).max(1) as u32 };
    let softver = najbolji(&radi, false);
    let hardver = najbolji(&radi, true);

    let trazeni = mode.trim().to_ascii_lowercase();
    let (odabran, mode_out, razlog) = match trazeni.as_str() {
        "cpu" | "procesor" => (
            softver,
            "cpu".to_string(),
            match softver {
                Some(probe) => format!("procesor: {} niti, {}", niti(cpu.threads), probe.encoder),
                None => "procesor: nema softverskog enkodera u ffmpeg-u".to_string(),
            },
        ),
        "gpu" | "graficka" | "grafička" => match hardver {
            Some(probe) => {
                (Some(probe), "gpu".to_string(), format!("grafička: {} ({} )", probe.encoder, probe.note))
            }
            None => (
                softver,
                "cpu".to_string(),
                "tražena grafička, ali nijedan HW enkoder ne radi — ide procesor".to_string(),
            ),
        },
        "hybrid" | "kombinirano" | "combo" => match hardver {
            Some(probe) => (
                Some(probe),
                "hybrid".to_string(),
                format!("kombinirano: HW dekodiranje + {} enkodiranje", probe.encoder),
            ),
            None => (
                softver,
                "hybrid".to_string(),
                "kombinirano: nema HW enkodera, dekodiranje ostaje softversko".to_string(),
            ),
        },
        _ => match (hardver, softver) {
            (Some(hw), Some(sw)) => {
                let hw_fps = hw.fps.unwrap_or(0.0);
                let sw_fps = sw.fps.unwrap_or(0.0);
                if hw_fps >= sw_fps {
                    (
                        Some(hw),
                        "gpu".to_string(),
                        format!(
                            "automatski: {} je brži ({:.0} vs {:.0} sličica/s)",
                            hw.encoder, hw_fps, sw_fps
                        ),
                    )
                } else {
                    (
                        Some(sw),
                        "cpu".to_string(),
                        format!("automatski: procesor je brži ({:.0} vs {:.0} sličica/s)", sw_fps, hw_fps),
                    )
                }
            }
            (Some(hw), None) => (Some(hw), "gpu".to_string(), format!("automatski: {}", hw.encoder)),
            (None, Some(sw)) => (
                Some(sw),
                "cpu".to_string(),
                format!("automatski: nema HW ubrzanja, {} u {} niti", sw.encoder, niti(cpu.threads)),
            ),
            (None, None) => (None, "cpu".to_string(), "nijedan enkoder ne radi".to_string()),
        },
    };

    let encoder = odabran.map(|probe| probe.encoder.clone()).unwrap_or_else(|| "libx264".to_string());
    let hardware = odabran.map(|probe| probe.hw != HwAccel::None).unwrap_or(false);
    let threads = if hardware { 0 } else { niti(cpu.threads) };
    let max_concurrent = if hardware { 2 } else { ((cpu.threads / 4).max(1)) as u32 };

    let (best_fps, other_fps) = match (odabran, softver, hardver) {
        (Some(izabran), Some(sw), Some(hw)) => {
            let drugi = if std::ptr::eq(izabran, sw) { hw } else { sw };
            (izabran.fps, drugi.fps)
        }
        (Some(izabran), _, _) => (izabran.fps, None),
        _ => (None, None),
    };

    Recommendation {
        ffmpeg_path,
        ffprobe_path,
        mode: mode_out,
        encoder,
        threads,
        hardware_decode: true,
        max_concurrent,
        reason: razlog,
        best_fps,
        other_fps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(encoder: &str, hw: HwAccel, fps: f64) -> EncoderProbe {
        EncoderProbe { encoder: encoder.to_string(), hw, works: true, fps: Some(fps), note: String::new() }
    }

    fn cpu(threads: usize) -> CpuInfo {
        CpuInfo { model: "test".to_string(), cores: threads, threads }
    }

    /// Stvarni sken na ovom stroju: NVENC (ako postoji) mora biti brži od libx264
    /// ili bar izmjeren — nikad `None` za enkoder koji radi.
    #[test]
    fn measured_encoders_all_have_a_speed() {
        let report = scan("ffmpeg", "ffprobe", "auto");
        for probe in report.encoders.iter().filter(|probe| probe.works) {
            assert!(probe.fps.unwrap_or(0.0) > 1.0, "{} radi, a nema brzinu: {:?}", probe.encoder, probe.fps);
        }
    }

    #[test]
    fn fps_parser_takes_the_last_value() {
        let ispis = "frame= 10 fps= 20 q=0.0\rframe= 30 fps= 90 q=0.0\rframe= 60 fps=145 q=0.0\nvideo:1kB";
        assert_eq!(parse_fps(ispis), Some(145.0), "zadnja vrijednost, i s razmakom i bez");
        assert_eq!(parse_fps("nema nista"), None);
    }

    #[test]
    fn auto_picks_the_faster_encoder() {
        let encoders =
            vec![probe("libx264", HwAccel::None, 180.0), probe("h264_nvenc", HwAccel::Nvenc, 640.0)];
        let preporuka = recommend(&[], &[], &cpu(16), &encoders, "auto");
        assert_eq!(preporuka.encoder, "h264_nvenc");
        assert_eq!(preporuka.mode, "gpu");
        assert_eq!(preporuka.threads, 0, "HW enkoder ne treba niti");
        assert!(preporuka.hardware_decode);
    }

    #[test]
    fn auto_can_prefer_the_processor_when_it_is_faster() {
        let encoders =
            vec![probe("libx264", HwAccel::None, 300.0), probe("h264_nvenc", HwAccel::Nvenc, 90.0)];
        let preporuka = recommend(&[], &[], &cpu(16), &encoders, "auto");
        assert_eq!(preporuka.encoder, "libx264");
        assert_eq!(preporuka.mode, "cpu");
        assert_eq!(preporuka.threads, 12, "tri četvrtine od 16 niti");
        assert!(!preporuka.hardware_decode || preporuka.encoder == "libx264");
    }

    #[test]
    fn cpu_mode_ignores_hardware() {
        let encoders =
            vec![probe("libx264", HwAccel::None, 100.0), probe("h264_nvenc", HwAccel::Nvenc, 900.0)];
        let preporuka = recommend(&[], &[], &cpu(8), &encoders, "cpu");
        assert_eq!(preporuka.encoder, "libx264");
        assert_eq!(preporuka.threads, 6);
        assert_eq!(preporuka.max_concurrent, 2);
    }

    #[test]
    fn gpu_mode_falls_back_to_cpu_with_a_clear_reason() {
        let encoders = vec![probe("libx264", HwAccel::None, 120.0)];
        let preporuka = recommend(&[], &[], &cpu(4), &encoders, "gpu");
        assert_eq!(preporuka.encoder, "libx264");
        assert!(preporuka.reason.contains("ne radi"), "{}", preporuka.reason);
        assert_eq!(preporuka.mode, "cpu");
    }

    #[test]
    fn h264_wins_when_hevc_is_only_slightly_faster() {
        let encoders = vec![
            probe("h264_nvenc", HwAccel::Nvenc, 148.0),
            probe("hevc_nvenc", HwAccel::Nvenc, 163.0),
            probe("libx264", HwAccel::None, 109.0),
        ];
        let preporuka = recommend(&[], &[], &cpu(8), &encoders, "auto");
        assert_eq!(preporuka.encoder, "h264_nvenc", "h264 iznad praga ide prije hevc-a");
    }

    #[test]
    fn hevc_wins_when_h264_is_too_slow() {
        // Slab stroj: h264 ne može ni jednu struju, hevc može — tada kompatibilnost čeka.
        let encoders = vec![probe("libx264", HwAccel::None, 22.0), probe("libx265", HwAccel::None, 58.0)];
        let preporuka = recommend(&[], &[], &cpu(8), &encoders, "auto");
        assert_eq!(preporuka.encoder, "libx265", "ispod praga ide najbrži");
    }

    #[test]
    fn broken_encoders_are_never_recommended() {
        let mut nvenc = probe("h264_nvenc", HwAccel::Nvenc, 0.0);
        nvenc.works = false;
        let encoders = vec![nvenc, probe("libx264", HwAccel::None, 150.0)];
        let preporuka = recommend(&[], &[], &cpu(8), &encoders, "auto");
        assert_eq!(preporuka.encoder, "libx264");
    }

    #[test]
    fn candidates_include_bundled_before_path() {
        let kandidati = tool_candidates("ffmpeg", "");
        assert!(kandidati.iter().any(|tool| tool.source == "priložen uz program"));
        let prvi = kandidati.first().expect("bar jedan kandidat");
        assert_eq!(prvi.source, "priložen uz program");
    }

    #[test]
    fn duplicate_paths_are_reported_once() {
        // Postavljena putanja i ista putanja iz PATH-a su jedan kandidat, ne dva.
        let kandidati = tool_candidates("ffmpeg", "/opt/homebrew/bin/ffmpeg");
        let broj = kandidati.iter().filter(|tool| tool.path == "/opt/homebrew/bin/ffmpeg").count();
        assert_eq!(broj, 1, "isti alat dva puta: {kandidati:?}");
    }

    /// Stvarni sken na ovom stroju — samo da ne paničari i da nešto nađe.
    #[test]
    fn real_scan_finds_something() {
        let report = scan("ffmpeg", "ffprobe", "auto");
        eprintln!(
            "ffmpeg kandidata: {}, enkodera: {}, GPU: {:?}, preporuka: {} ({})",
            report.ffmpeg.len(),
            report.encoders.iter().filter(|probe| probe.works).count(),
            report.gpus.iter().map(|gpu| gpu.name.clone()).collect::<Vec<_>>(),
            report.recommendation.encoder,
            report.recommendation.reason
        );
        assert!(!report.recommendation.encoder.is_empty());
        assert!(!report.cpu.model.is_empty());
    }
}
