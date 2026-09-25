//! Konfiguracija: TOML na platformskoj putanji, s razumnim defaultima i
//! pogadanjem pocetnih medijskih mapa pri prvom pokretanju.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{DEFAULT_HTTP_PORT, DEFAULT_MAX_AGE};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerSection,
    pub library: LibrarySection,
    pub transcode: TranscodeSection,
    pub profiles: ProfilesSection,
    pub network: NetworkSection,
    /// Web sucelje (jezik sucelja). Zadano `auto` — prati jezik browsera.
    pub ui: UiSection,
}

/// Postavke web sucelja.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UiSection {
    /// `auto` (prati browser), `hr` ili `en`.
    #[serde(default = "default_language")]
    pub language: String,
}

impl Default for UiSection {
    fn default() -> Self {
        Self { language: default_language() }
    }
}

/// Zadani jezik sučelja: hrvatski.
///
/// Namjerno nije `auto` — sučelje mora izgledati isto u desktop aplikaciji i u
/// browseru, a `auto` bi u jednom prozoru dao hrvatski, a u drugom engleski
/// (svaki prati svoj `navigator.language`). Korisnik prebaci u Postavkama.
pub fn default_language() -> String {
    "hr".to_string()
}

impl Config {
    /// Javi gdje se stvarno nalaze ffmpeg/ffprobe.
    ///
    /// Prilozeni uz binarni fajl imaju prednost pred `PATH`-om, pa paket radi
    /// odmah po instaliranju i na racunalu bez ffmpeg-a.
    pub fn resolve_tools(&mut self) -> (String, String) {
        let ffmpeg = crate::tools::resolve(&self.transcode.ffmpeg_path);
        let ffprobe = crate::tools::resolve(&self.transcode.ffprobe_path);
        if ffmpeg != self.transcode.ffmpeg_path || ffprobe != self.transcode.ffprobe_path {
            tracing::info!(ffmpeg = %ffmpeg, ffprobe = %ffprobe, "koristim prilozene alate uz program");
        }
        self.transcode.ffmpeg_path = ffmpeg.clone();
        self.transcode.ffprobe_path = ffprobe.clone();
        (ffmpeg, ffprobe)
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }

    /// Ucita config ili vrati default (bez pisanja na disk).
    pub fn load_or_default(path: &Path) -> anyhow::Result<Self> {
        if path.exists() { Self::load(path) } else { Ok(Self::default()) }
    }

    /// Ucita config, a ako ne postoji — napise default i vrati ga.
    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            return Self::load(path);
        }
        let cfg = Self::default();
        cfg.save(path)?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn http_port(&self) -> u16 {
        self.server.http_port
    }

    /// Mape koje postoje na disku (skener preskace ostale).
    pub fn existing_roots(&self) -> Vec<Root> {
        self.library.roots.iter().filter(|r| r.path.is_dir()).cloned().collect()
    }
}

/// Mreza: koja se adresa koristi za dohvat s interneta.
///
/// Zasto postoji: kucne mreze cesto imaju IPv6 koji "izgleda" dostupan (DNS
/// vrati AAAA), ali ne vodi nikamo — zahtjev tada visi dok ne istekne vrijeme.
/// Zato je zadano `ipv4`; `any` vrati staro ponasanje.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkSection {
    /// `ipv4` (zadano), `ipv6` ili `any`.
    #[serde(default = "default_ip_family")]
    pub ip_family: String,
}

fn default_ip_family() -> String {
    "ipv4".to_string()
}

impl Default for NetworkSection {
    fn default() -> Self {
        Self { ip_family: default_ip_family() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerSection {
    /// Ime koje se vidi na TV-u; `None` -> "Rustiio (<hostname>)".
    pub friendly_name: Option<String>,
    /// Adresa na koju se veze HTTP server.
    pub bind: String,
    pub http_port: u16,
    /// IP koji se oglasava u LOCATION URL-u; `None` -> auto detekcija.
    pub advertise_ip: Option<String>,
    /// Perzistirani UPnP UDN.
    pub udn: Option<String>,
    pub max_age_secs: u32,
    /// Salji SSDP announce (iskljuci u testovima).
    pub ssdp: bool,
    /// Razina loga: trace|debug|info|warn|error.
    pub log_level: String,
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            friendly_name: None,
            bind: "0.0.0.0".to_string(),
            http_port: DEFAULT_HTTP_PORT,
            advertise_ip: None,
            udn: None,
            max_age_secs: DEFAULT_MAX_AGE,
            ssdp: true,
            log_level: "info".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LibrarySection {
    pub roots: Vec<Root>,
    /// Ekstenzije koje smatramo videom.
    pub video_extensions: Vec<String>,
    /// Koliko duboko skeniramo (zastita od beskonacnih stabala).
    pub max_depth: u32,
    /// Virtualne kategorije na vrhu (Video / Muzika / Slike / Nedavno dodano).
    pub views: bool,
    /// Koje se virtualne kategorije nude na televizorima: `movies`, `series`,
    /// `video`, `recent`, `audio`, `image` (može i `v:movies`). Prazno → filmovi i serije.
    #[serde(default)]
    pub view_list: Vec<String>,
    /// Koliko objekata ide u "Nedavno dodano".
    pub recent_limit: u32,
    /// Dohvacaj postera u pozadini (uz datoteku, pa TMDB/Wikipedia/TVmaze/Cover Art).
    pub posters: bool,
    /// Prati mape i sam pokreni sken kad se datoteka doda ili obrise.
    pub watch: bool,
    /// Auto-sken: minute izmedju dva skeniranja (`None` = 15, `0` = iskljuceno).
    pub auto_scan_minutes: Option<u64>,

}

impl Default for LibrarySection {
    fn default() -> Self {
        Self {
            roots: guess_roots(),
            video_extensions: default_video_extensions(),
            max_depth: 8,
            views: true,
            view_list: Vec::new(),
            recent_limit: 20,
            posters: true,
            watch: true,
            auto_scan_minutes: None,
        }
    }
}

/// Jedna medijska mapa. `label` je ime koje TV vidi.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Root {
    #[serde(default)]
    pub label: String,
    pub path: PathBuf,
    #[serde(default)]
    pub kind: RootKind,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RootKind {
    Video,
    Audio,
    Image,
    #[default]
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscodeSection {
    /// Ako je false, Rustiio nikad ne poziva ffmpeg (direct play + remux).
    pub enabled: bool,
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
    /// auto | none | nvenc | vaapi | qsv | videotoolbox | amf
    pub hw_accel: String,
    pub max_concurrent: u32,
    /// Koliko sekundi unaprijed ffmpeg smije ici prije gledatelja (buffering).
    pub buffer_secs: u32,
    /// Ocituj trajanje preko ffprobe (potrebno za `TimeSeekRange`).
    pub probe_duration: bool,
    /// Konkretan enkoder (`h264_nvenc`, `libx264`, ...); prazno = najbolji dostupni.
    #[serde(default)]
    pub encoder: String,
    /// Način rada koji je korisnik odabrao: auto | gpu | cpu | hybrid.
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Niti koje softverski enkoder smije koristiti (0 = sve jezgre).
    #[serde(default)]
    pub threads: u32,
    /// Hardversko dekodiranje (GPU) — vrijedi i kad enkodira procesor.
    #[serde(default = "default_true")]
    pub hardware_decode: bool,
}

fn default_mode() -> String {
    "auto".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for TranscodeSection {
    fn default() -> Self {
        Self {
            enabled: true,
            ffmpeg_path: "ffmpeg".to_string(),
            ffprobe_path: "ffprobe".to_string(),
            hw_accel: "auto".to_string(),
            max_concurrent: 2,
            buffer_secs: 30,
            probe_duration: true,
            encoder: String::new(),
            mode: default_mode(),
            threads: 0,
            hardware_decode: default_true(),
        }
    }
}

/// Profili uredjaja: koji TV sto moze i kako mu se salje.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfilesSection {
    /// Mapa s korisnickim `*.toml` profilima. Prazno = `<config_dir>/profiles`.
    /// Profil s istim `id`-om pregazi ugradjeni.
    pub dir: PathBuf,
    /// Biljezi zaglavlja svakog uredjaja koji nesto trazi (za pravljenje profila).
    pub capture: bool,
}

impl Default for ProfilesSection {
    fn default() -> Self {
        Self { dir: PathBuf::new(), capture: true }
    }
}

pub fn default_video_extensions() -> Vec<String> {
    [
        "mkv", "mp4", "m4v", "avi", "mov", "ts", "m2ts", "mts", "webm", "mpg", "mpeg", "wmv", "flv", "divx",
        "vob", "ogv",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Mape u kojima se na svakoj platformi najcesce nalaze filmovi/serije.
pub fn guess_roots() -> Vec<Root> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = home_dir() {
        candidates.push(home.join("Movies"));
        candidates.push(home.join("Videos"));
        candidates.push(home.join("Downloads"));
    }
    candidates.push(PathBuf::from("/media"));
    candidates.push(PathBuf::from("/mnt/media"));
    candidates.push(PathBuf::from("D:\\Media"));

    let mut out = Vec::new();
    for path in candidates {
        if out.len() >= 4 || !path.is_dir() {
            continue;
        }
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        out.push(Root { label, path, kind: RootKind::Video });
    }
    out
}

/// Konfiguracijska mapa (postuje `RUSTIIO_CONFIG_DIR`).
pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RUSTIIO_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    if cfg!(target_os = "windows") {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("Rustiio");
        }
    }
    if cfg!(target_os = "macos") {
        if let Some(home) = home_dir() {
            return home.join("Library/Application Support/Rustiio");
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("rustiio");
    }
    home_dir().map(|h| h.join(".config/rustiio")).unwrap_or_else(|| PathBuf::from("/etc/rustiio"))
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrips_through_toml() {
        let cfg = Config::default();
        let text = toml::to_string_pretty(&cfg).expect("serialize");
        let back: Config = toml::from_str(&text).expect("deserialize");
        assert_eq!(back.server.http_port, DEFAULT_HTTP_PORT);
        assert_eq!(back.library.video_extensions, cfg.library.video_extensions);
        assert!(back.transcode.enabled);
    }

    #[test]
    fn partial_config_gets_defaults() {
        let cfg: Config = toml::from_str("[server]\nhttp_port = 9000\n").expect("parse");
        assert_eq!(cfg.server.http_port, 9000);
        assert_eq!(cfg.server.max_age_secs, DEFAULT_MAX_AGE);
        assert!(!cfg.library.video_extensions.is_empty());
    }

    #[test]
    fn config_save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("rustiio-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("config.toml");
        let mut cfg = Config::default();
        cfg.server.friendly_name = Some("Test TV".to_string());
        cfg.save(&path).expect("save");
        let back = Config::load(&path).expect("load");
        assert_eq!(back.server.friendly_name.as_deref(), Some("Test TV"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
