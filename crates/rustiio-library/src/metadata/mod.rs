//! Posteri i metapodaci: red izvora koji pada s jednog na drugi.
//!
//! Redoslijed je namjeran — prvo ono što je lokalno i točno, pa službeni API
//! ako imamo ključ, pa keyless javni izvori:
//!
//! 1. `poster.jpg` / `folder.jpg` / `<film>.jpg` uz datoteku (vidi `local`)
//! 2. TMDB s ključem (API v3 ili v4 token)
//! 3. TMDB bez ključa (web stranica + `media.themoviedb.org`)
//! 4. Wikipedia (`pageimages` s `pilicense=any` — bez toga posteri ne izlaze)
//! 5. TVmaze (serije), Cover Art Archive (glazba)
//!
//! Ništa se ne dohvaća samo zato što može: bez pronađenog postera nema upisa.

pub mod cache;
pub mod keyless;
pub mod local;
pub mod pacing;
pub mod tmdb;

use std::path::PathBuf;

/// Što tražimo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Očišćen naslov ("Sicario", "Dark Matter").
    pub title: String,
    /// Godina ako se da izvući iz imena datoteke.
    pub year: Option<u32>,
    /// Serija ili film (TVmaze radi samo serije).
    pub is_series: bool,
}

/// Odakle je poster došao.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Local,
    Tmdb,
    TmdbWeb,
    Wikipedia,
    Tvmaze,
    CoverArt,
}

impl Source {
    /// Kratko ime za log i bazu.
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Local => "local",
            Source::Tmdb => "tmdb",
            Source::TmdbWeb => "tmdb-web",
            Source::Wikipedia => "wikipedia",
            Source::Tvmaze => "tvmaze",
            Source::CoverArt => "coverart",
        }
    }
}

/// Nađeni poster (još nije na disku).
#[derive(Debug, Clone)]
pub struct Found {
    pub source: Source,
    pub url: String,
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

/// Poster koji je završio u kešu (ili je već tamo bio).
#[derive(Debug, Clone)]
pub struct Poster {
    pub path: PathBuf,
    /// Odakle je došao; `None` znači da je već bio u kešu.
    pub source: Option<Source>,
    pub bytes: usize,
}

/// Red izvora: lokalno → TMDB (ključ ili web) → Wikipedia → TVmaze → Cover Art Archive.
pub struct Enricher {
    agent: ureq::Agent,
    api_key: Option<String>,
    art_dir: PathBuf,
    width: String,
}

impl Enricher {
    /// `art_dir` je mapa s kešom (`<config_dir>/art`).
    ///
    /// `ip_family` je tekst iz configa (`ipv4` / `ipv6` / `any`). Na mrezama s
    /// polomljenim IPv6-om `ipv4` je razlika izmedju "radi" i "visi 20 s po
    /// zahtjevu", pa se vrijednost logira pri stvaranju.
    pub fn new(api_key: Option<String>, art_dir: PathBuf, ip_family: &str) -> Self {
        let family = parse_ip_family(ip_family);
        tracing::info!(ip_family = ?family, "mreza za dohvat postera");
        let config = ureq::Agent::config_builder()
            .ip_family(family)
            // Kratak connect timeout: mrtva adresa ne smije pojesti cijeli zahtjev.
            .timeout_connect(Some(std::time::Duration::from_secs(4)))
            .timeout_global(Some(std::time::Duration::from_secs(20)))
            .user_agent(keyless::USER_AGENT)
            .build();
        let normalized = api_key.map(|key| key.trim().to_string()).filter(|key| !key.is_empty());
        Self { agent: config.into(), api_key: normalized, art_dir, width: tmdb::DEFAULT_WIDTH.to_string() }
    }

    /// Ključ iz okoline (`TMDB_API_KEY`); prazno znači "radi bez ključa".
    pub fn from_env(art_dir: PathBuf, ip_family: &str) -> Self {
        Self::new(std::env::var("TMDB_API_KEY").ok(), art_dir, ip_family)
    }

    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    pub fn art_dir(&self) -> &std::path::Path {
        &self.art_dir
    }

    /// Poster za konkretnu video datoteku: keš → uz datoteku → mreža.
    pub fn poster_for(&self, item_id: i64, video: &std::path::Path) -> Option<Poster> {
        if let Some(path) = cache::existing(&self.art_dir, item_id) {
            return Some(Poster { bytes: 0, path, source: None });
        }
        if let Some((found_at, bytes)) = local::find_local(video) {
            let extension = local::extension_of(&bytes);
            let path = cache::store(&self.art_dir, item_id, extension, &bytes).ok()?;
            tracing::info!(id = item_id, from = %found_at.display(), "poster uz datoteku");
            return Some(Poster { path, source: Some(Source::Local), bytes: bytes.len() });
        }
        let stem = video.file_stem()?.to_str()?;
        self.poster_for_query(item_id, &guess_title(stem))
    }

    /// Poster za naslov (film, serija ili album).
    pub fn poster_for_query(&self, item_id: i64, query: &Query) -> Option<Poster> {
        if query.title.trim().is_empty() {
            return None;
        }
        if let Some(path) = cache::existing(&self.art_dir, item_id) {
            return Some(Poster { bytes: 0, path, source: None });
        }

        let found = tmdb::poster(&self.agent, query, self.api_key.as_deref(), &self.width)
            .or_else(|| keyless::wikipedia_poster(&self.agent, query))
            .or_else(|| keyless::tvmaze_poster(&self.agent, query))
            .or_else(|| keyless::coverart_poster(&self.agent, query))?;

        if !cache::is_image(&found.bytes) {
            tracing::warn!(title = %query.title, url = %found.url, "dohvaceno nije slika");
            return None;
        }
        let path = cache::store(&self.art_dir, item_id, found.extension, &found.bytes).ok()?;
        tracing::info!(
            id = item_id,
            title = %query.title,
            source = found.source.as_str(),
            bytes = found.bytes.len(),
            "poster dohvacen"
        );
        Some(Poster { path, source: Some(found.source), bytes: found.bytes.len() })
    }
}

/// Očisti ime datoteke u naslov i godinu.
///
/// Ulaz su imena kakva stvarno dolaze s diska:
/// `Sicario.2015.1080p.BluRay.x264-[YTS.AM].mkv`, `Dark.Matter.S02E03.1080p.x265-ELiTE.mkv`,
/// `Zestoki.Decki.2016.HRTV.mp4`.
pub fn guess_title(stem: &str) -> Query {
    // Podnesi i puno ime datoteke i samo stem.
    let without_extension = match stem.rsplit_once('.') {
        Some((head, tail))
            if !head.is_empty()
                && matches!(
                    tail.to_lowercase().as_str(),
                    "mkv"
                        | "mp4"
                        | "avi"
                        | "m4v"
                        | "mov"
                        | "ts"
                        | "mpg"
                        | "mpeg"
                        | "flv"
                        | "webm"
                        | "mp3"
                        | "flac"
                        | "m4a"
                        | "ogg"
                ) =>
        {
            head
        }
        _ => stem,
    };
    let normalized = without_extension.replace(['_', '.'], " ");
    let mut is_series = false;
    let mut year = None;
    let mut title_words: Vec<&str> = Vec::new();

    for word in normalized.split_whitespace() {
        let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric());
        let lower = trimmed.to_lowercase();

        // Oznaka epizode: S01E03 / s01e03 / 1x03.
        if rustiio_library_series_episode(&lower) {
            is_series = true;
            continue;
        }
        if year.is_none() && trimmed.len() == 4 && trimmed.chars().all(|c| c.is_ascii_digit()) {
            let parsed: u32 = trimmed.parse().unwrap_or(0);
            if (1900..=2100).contains(&parsed) {
                year = Some(parsed);
                continue;
            }
        }
        // Nakon godine u imenu datoteke dolazi samo izdanje/kanal (`2016 HRTV`),
        // osim ako je godina ujedno i cijeli naslov (`1917`).
        if year.is_some() && !title_words.is_empty() {
            continue;
        }
        if is_noise(&lower) {
            continue;
        }
        title_words.push(trimmed);
    }

    Query { title: title_words.join(" ").trim().to_string(), year, is_series }
}

/// `S01E03`, `s1e3`, `1x03` — ista logika kao u `series`, ali bez ovisnosti o njoj.
fn rustiio_library_series_episode(word: &str) -> bool {
    let lower = word.to_lowercase();
    if let Some((season, rest)) = lower.split_once('x') {
        if !season.is_empty()
            && season.chars().all(|c| c.is_ascii_digit())
            && !rest.is_empty()
            && rest.chars().all(|c| c.is_ascii_digit())
        {
            return true;
        }
    }
    let Some(start) = lower.find('s') else { return false };
    let rest = &lower[start + 1..];
    let Some(episode_at) = rest.find('e') else { return false };
    let season = &rest[..episode_at];
    let episode = &rest[episode_at + 1..];
    !season.is_empty()
        && !episode.is_empty()
        && season.chars().all(|c| c.is_ascii_digit())
        && episode.chars().all(|c| c.is_ascii_digit())
}

/// Release-šum koji ne ide u naslov.
fn is_noise(word: &str) -> bool {
    const NOISE: [&str; 24] = [
        "1080p", "720p", "2160p", "480p", "4k", "uhd", "bluray", "blu", "ray", "webrip", "web", "dl",
        "hdrip", "brrip", "dvdrip", "x264", "x265", "h264", "h265", "hevc", "aac", "ac3", "dts", "remux",
    ];
    if NOISE.contains(&word) {
        return true;
    }
    // `x265-ELiTE`, `h264-successfulcrab` — grupa je zalijepljena za oznaku kodeka.
    if let Some((codec, _group)) = word.split_once('-') {
        if NOISE.contains(&codec) {
            return true;
        }
    }
    // Kanal/grupa: `[YTS.AM]`, `-ELiTE`, `EZTVx.to`, `www.YTS.MX`.
    word.contains('.')
        || word.contains('[')
        || word.contains(']')
        || word.starts_with("www")
        || word.ends_with("mx")
        || word.ends_with("am")
}

/// Tekst iz configa u obitelj adresa (`ipv4` je zadano).
pub fn parse_ip_family(value: &str) -> ureq::config::IpFamily {
    match value.trim().to_ascii_lowercase().as_str() {
        "ipv4" | "4" | "v4" => ureq::config::IpFamily::Ipv4Only,
        "ipv6" | "6" | "v6" => ureq::config::IpFamily::Ipv6Only,
        _ => ureq::config::IpFamily::Any,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_movie_file_names() {
        let query = guess_title("Sicario.2015.1080p.BluRay.x264-[YTS.AM]");
        assert_eq!(query.title, "Sicario");
        assert_eq!(query.year, Some(2015));
        assert!(!query.is_series);

        let hr = guess_title("Zestoki.Decki.2016.HRTV.mp4");
        assert_eq!(hr.title, "Zestoki Decki");
        assert_eq!(hr.year, Some(2016));
    }

    #[test]
    fn recognizes_series_and_keeps_title() {
        let query = guess_title("Dark.Matter.S02E03.1080p.x265-ELiTE");
        assert_eq!(query.title, "Dark Matter");
        assert!(query.is_series);
        assert_eq!(query.year, None);

        let other = guess_title("dark.matter.2024.s01e06.1080p.web.h264-successfulcrab");
        assert_eq!(other.title, "dark matter");
        assert!(other.is_series);
        assert_eq!(other.year, Some(2024));

        assert!(guess_title("Serija 1x03.avi").is_series);
    }

    #[test]
    fn title_without_noise_stays_untouched() {
        let query = guess_title("Test Film (2026)");
        assert_eq!(query.title, "Test Film");
        assert_eq!(query.year, Some(2026));
    }
}
