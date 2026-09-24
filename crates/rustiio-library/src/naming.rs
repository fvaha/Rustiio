//! Raspoznavanje naziva datoteka: serija, sezona, epizoda, godina.
//!
//! Zašto ovdje, a ne u sučelju: televizor i web prikazuju **isti** katalog, pa se
//! imena moraju očistiti jednom, pri skenu, da se oba ne mogu razići.
//!
//! Ulaz su imena kakva stvarno dolaze s trackera:
//! `furious.s01e01.1080p.cakes[EZTVx.to]`,
//! `www.UIndex.org - Dark Matter S02E05 1080p WEBRip 10Bit DDP5 1 HEVC-d3g`,
//! `Escape at Dannemora (2018) Season 1 S01 (1080p AMZN WEB-DL x265 HEVC 10bit …)`.

/// Vrsta naziva — što je datoteka po imenu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    /// Epizoda serije (ima sezonu i/ili epizodu).
    Episode,
    /// Film ili datoteka bez oznake epizode.
    Movie,
}

/// Očišćen naziv i oznake izvučene iz njega.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedName {
    /// Za prikaz: `Dark Matter`.
    pub title: String,
    /// Za grupiranje: `dark matter` (mala slova, bez interpunkcije).
    pub key: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub year: Option<u32>,
    /// Kvaliteta za prikaz (`1080p`), ako je u nazivu bila.
    pub quality: Option<String>,
    pub kind: NameKind,
}

impl ParsedName {
    /// Oznaka epizode za prikaz: `S02E05` (dvoznamenkasto, pa se i abecedno slaže).
    pub fn label(&self) -> String {
        match (self.season, self.episode) {
            (Some(season), Some(episode)) => format!("S{season:02}E{episode:02}"),
            (Some(season), None) => format!("S{season:02}"),
            (None, Some(episode)) => format!("S00E{episode:02}"),
            (None, None) => self.title.clone(),
        }
    }

    /// Naslov epizode onako kako ide u katalog: `S02E05 · 1080p`.
    pub fn display(&self) -> String {
        let label = self.label();
        match &self.quality {
            Some(quality) if self.kind == NameKind::Episode => format!("{label} · {quality}"),
            _ => label,
        }
    }

    pub fn is_episode(&self) -> bool {
        self.kind == NameKind::Episode
    }
}

/// Riječi koje nisu dio naslova (izdanje, kvaliteta, zvuk, jezik, grupa).
const JUNK: &[&str] = &[
    // kvaliteta i izvor
    "480p",
    "576p",
    "720p",
    "1080p",
    "1080i",
    "2160p",
    "4k",
    "8k",
    "uhd",
    "fhd",
    "hd",
    "sd",
    "hq",
    "ts",
    "tc",
    "web",
    "webdl",
    "webrip",
    "web-dl",
    "web-rip",
    "bluray",
    "blu-ray",
    "bdrip",
    "brrip",
    "bdremux",
    "remux",
    "dvdrip",
    "dvdscr",
    "dvd",
    "hdtv",
    "hdrip",
    "pdtv",
    "dsr",
    "cam",
    "telesync",
    "screener",
    "amzn",
    "dsnp",
    "atvp",
    "itunes",
    "nf",
    "max",
    "hmax",
    "pcok",
    "stan",
    "itu",
    "now",
    "hulu",
    "pcmp",
    "yts",
    "yify",
    // kodek i bitovi
    "x264",
    "x265",
    "h264",
    "h265",
    "h 264",
    "h 265",
    "hevc",
    "avc",
    "xvid",
    "divx",
    "av1",
    "vp9",
    "mpeg2",
    "mpeg4",
    "10bit",
    "8bit",
    "hi10p",
    "10-bit",
    "8-bit",
    "4bit",
    // zvuk
    "aac",
    "aac2",
    "ac3",
    "eac3",
    "e-ac3",
    "dts",
    "dtshd",
    "dts-hd",
    "dd",
    "ddp",
    "dd5",
    "ddp5",
    "truehd",
    "atmos",
    "flac",
    "mp3",
    "opus",
    "5 1",
    "7 1",
    "2 0",
    "6ch",
    "8ch",
    // jezici
    "eng",
    "english",
    "ita",
    "italian",
    "ger",
    "german",
    "deu",
    "fre",
    "french",
    "fra",
    "spa",
    "spanish",
    "hun",
    "nor",
    "norwegian",
    "swe",
    "swedish",
    "dan",
    "danish",
    "fin",
    "finnish",
    "dut",
    "nl",
    "rus",
    "pol",
    "tur",
    "ara",
    "hin",
    "jpn",
    "kor",
    "chi",
    "zho",
    "hrv",
    "srp",
    "ces",
    "cze",
    "por",
    "brazilian",
    "multi",
    "dual",
    "subs",
    "sub",
    "dub",
    "hc",
    "vostfr",
    "truefrench",
    // izdanja i ostalo
    "internal",
    "proper",
    "repack",
    "extended",
    "unrated",
    "remastered",
    "complete",
    "imax",
    "hdr",
    "hdr10",
    "dovi",
    "dv",
    "sdr",
    "ds4k",
    "divx",
    "ws",
    "fs",
];

/// Vrste koje se odbacuju samo ako stoje **prije** glavnog naslova (smeće sprijeda).
const LEADING_JUNK: &[&str] = &["www", "uindex", "org", "com", "net", "http", "https", "wwwuindexorg"];

/// Složi riječi u tekst s razmacima (`["Dark","Matter"]` → `Dark Matter`).
fn join(words: &[String]) -> String {
    words.join(" ").trim().to_string()
}

/// Normalizira za usporedbu: mala slova, bez interpunkcije, bez dvostrukih razmaka.
pub fn key_of(words: &[String]) -> String {
    let raw = join(words).to_lowercase();
    let mut out = String::with_capacity(raw.len());
    let mut space = true;
    for ch in raw.chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim().to_string()
}

/// Je li token godina (`2019`) i vrati je.
pub fn year_of(token: &str) -> Option<u32> {
    if token.len() == 4 {
        if let Ok(year) = token.parse::<u32>() {
            if (1900..=2099).contains(&year) {
                return Some(year);
            }
        }
    }
    None
}

fn is_year(token: &str) -> Option<u32> {
    year_of(token)
}

/// Godina u zagradama: `The Movie (2019)`, `Film [2018]`.
///
/// `strip_brackets` briše zagrade kao smeće, pa bi bez ovoga film ostao bez godine.
fn year_in_brackets(raw: &str) -> Option<u32> {
    let mut rest = raw;
    while let Some(start) = rest.find(['(', '[']) {
        let after = &rest[start + 1..];
        let end = after.find([')', ']']).unwrap_or(after.len());
        if let Some(year) = year_of(after[..end].trim()) {
            return Some(year);
        }
        rest = &after[end.min(after.len())..];
    }
    None
}

fn is_junk(token: &str) -> bool {
    let lower = token.to_lowercase();
    if lower.is_empty() {
        return true;
    }
    if JUNK.contains(&lower.as_str()) {
        return true;
    }
    // `DDP5`, `DD+5`, `EAC35` i slične slijepljene oznake zvuka.
    let stripped: String = lower.chars().filter(|ch| ch.is_ascii_alphabetic()).collect();
    let audio = ["ddp", "dd", "eac", "dts", "aac", "ac", "truehd"];
    if audio.contains(&stripped.as_str()) && lower.chars().any(|ch| ch.is_ascii_digit()) {
        return true;
    }
    false
}

/// Skini `[EZTVx.to]`, `(1080p)` i slične zagrade.
fn strip_brackets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0i32;
    for ch in text.chars() {
        match ch {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Podijeli na riječi: točke, podvlake i crte su granice (`Slow.Horses.S01E01`).
fn words_of(raw: &str) -> Vec<String> {
    raw.split(|ch: char| ch == '.' || ch == '_' || ch == '-' || ch.is_whitespace())
        .map(|piece| piece.trim().to_string())
        .filter(|piece| !piece.is_empty())
        .collect()
}

/// Nađi oznaku epizode/sezone u riječima; vraća (indeks, sezona, epizoda, broj riječi oznake).
fn find_episode_marker(words: &[String]) -> Option<(usize, Option<u32>, Option<u32>, usize)> {
    for (index, word) in words.iter().enumerate() {
        let lower = word.to_lowercase();

        // 1x02
        if let Some((left, right)) = lower.split_once('x') {
            if !left.is_empty() && !right.is_empty() {
                if let (Ok(season), Ok(episode)) = (left.parse::<u32>(), right.parse::<u32>()) {
                    if season <= 40 && episode <= 999 {
                        return Some((index, Some(season), Some(episode), 1));
                    }
                }
            }
        }

        // S01E02, s01.e02 (već spojeno u jednu riječ)
        if let Some(rest) = lower.strip_prefix('s') {
            if let Some((season_text, episode_text)) = rest.split_once('e') {
                let season = season_text.parse::<u32>().ok();
                let episode = episode_text
                    .chars()
                    .take_while(|ch| ch.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u32>()
                    .ok();
                if let (Some(season), Some(episode)) = (season, episode) {
                    return Some((index, Some(season), Some(episode), 1));
                }
            }
            // Samo sezona: S01 (ali ne "S01E" ni riječ tipa "Series")
            if let Ok(season) = rest.parse::<u32>() {
                if season <= 40 && rest.len() <= 2 {
                    // Epizoda može stajati iza: `S01.E02`, `S01 Episode 5`, `S01 E05`.
                    let episode = episode_after_season(&words[index + 1..]);
                    return Some((index, Some(season), episode, 1));
                }
            }
        }

        // E02 (bez sezone — sezona se traži u mapi iznad)
        if let Some(episode_text) = lower.strip_prefix('e') {
            if let Ok(episode) = episode_text.parse::<u32>() {
                if episode <= 999 {
                    return Some((index, None, Some(episode), 1));
                }
            }
        }

        // "Episode 3", "Ep. 3", "Sezona 2"
        let word_lower = lower.trim_end_matches('.');
        if word_lower == "episode" || word_lower == "ep" || word_lower == "e" {
            if let Some(Ok(episode)) = words.get(index + 1).map(|next| next.parse::<u32>()) {
                return Some((index, None, Some(episode), 2));
            }
        }
        if word_lower == "season" || word_lower == "sezona" || word_lower == "saison" {
            if let Some(Ok(season)) = words.get(index + 1).map(|next| next.parse::<u32>()) {
                // Broj iza "Season" je sezona, epizoda (ako je) ide dalje iza njega.
                let episode = episode_after_season(&words[(index + 2).min(words.len())..]);
                return Some((index, Some(season), episode, 2));
            }
        }
    }
    None
}

/// Epizoda koja stoji **iza** oznake sezone: `Episode 5`, `Epizoda 5`, `E05`, `05`.
fn episode_after_season(words: &[String]) -> Option<u32> {
    let mut index = 0;
    while index < words.len() {
        let lower = words[index].to_lowercase();
        let lower = lower.trim_end_matches('.').to_string();
        // `E05`
        if let Some(digits) = lower.strip_prefix('e') {
            if let Ok(episode) = digits.parse::<u32>() {
                if episode <= 999 {
                    return Some(episode);
                }
            }
        }
        // `Episode 5` / `Epizoda 5` — broj stoji u sljedećoj riječi.
        if ["episode", "epizoda", "epizode", "ep"].contains(&lower.as_str()) {
            if let Some(Ok(episode)) = words.get(index + 1).map(|next| next.parse::<u32>()) {
                return Some(episode);
            }
        }
        // Goli broj (`S01 05`), ali ne godina i ne oznaka kvalitete.
        if let Ok(number) = lower.parse::<u32>() {
            if number <= 999 && is_year(&lower).is_none() && !is_junk(&lower) {
                return Some(number);
            }
        }
        index += 1;
    }
    None
}

/// Očisti riječi u naslov: bez smeća, bez godine, bez praznina.
fn clean_title(words: &[String]) -> (String, Option<u32>) {
    let mut kept: Vec<String> = Vec::new();
    let mut year = None;
    for (index, word) in words.iter().enumerate() {
        let Some(year_value) = is_year(word) else {
            // Vodeći URL/tracker ostaci idu van, ali samo dok naslov još ne počinje.
            if kept.is_empty() && LEADING_JUNK.contains(&word.to_lowercase().as_str()) {
                continue;
            }
            if is_junk(word) {
                continue;
            }
            kept.push(word.clone());
            continue;
        };
        // Godina: ako je zadnja riječ naslova, samo se zabilježi i izbaci; ako iza nje
        // ima još riječi, to je početak smeća (`2018 Season 1 S01 1080p …`).
        year = Some(year_value);
        let rest_have_title = words[index + 1..].iter().any(|next| !is_junk(next) && is_year(next).is_none());
        if rest_have_title {
            break;
        }
    }

    // Ako je sve otpalo (`1080p.mkv`), vrati original da naziv ne bude prazan.
    if kept.is_empty() {
        kept = words.iter().filter(|word| is_year(word).is_none()).cloned().collect();
    }

    let mut title = join(&kept);
    // `furious` → `Furious`; naslove s velikim slovima ne diramo.
    if !title.is_empty() && title.chars().all(|ch| !ch.is_alphabetic() || ch.is_lowercase()) {
        title = title
            .split(' ')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
    }
    (title, year)
}

fn quality_of(words: &[String]) -> Option<String> {
    const QUALITY: &[&str] = &["2160p", "1080p", "1080i", "720p", "576p", "480p", "4k"];
    words.iter().map(|word| word.to_lowercase()).find(|word| QUALITY.contains(&word.as_str()))
}

/// Raspoznaj naziv datoteke ili mape.
pub fn parse(raw: &str) -> ParsedName {
    let without_extension = raw
        .rsplit_once('.')
        .map(|(head, tail)| if tail.len() <= 4 && !tail.contains(' ') { head } else { raw })
        .unwrap_or(raw);
    let cleaned = strip_brackets(without_extension);
    let words = words_of(&cleaned);
    let quality = quality_of(&words);

    match find_episode_marker(&words) {
        Some((index, season, episode, _)) => {
            let (title, year) = clean_title(&words[..index]);
            let title = if title.is_empty() || title.chars().all(|ch| ch.is_ascii_digit()) {
                // Naziv je samo u mapi iznad (npr. `S01E02.mkv`) — nosi ga pozivnik.
                String::new()
            } else {
                title
            };
            let key = key_of(&words_of(&title));
            let year = year.or_else(|| year_in_brackets(raw));
            ParsedName { title, key, season, episode, year, quality, kind: NameKind::Episode }
        }
        None => {
            let (title, year) = clean_title(&words);
            let key = key_of(&words_of(&title));
            let year = year.or_else(|| year_in_brackets(raw));
            ParsedName { title, key, season: None, episode: None, year, quality, kind: NameKind::Movie }
        }
    }
}

/// Redni broj iz imena datoteke (`03.mkv` → 3, `Epizoda 12` → 12) — kad oznake nema.
pub fn episode_number_hint(raw: &str) -> Option<u32> {
    let words = words_of(&strip_brackets(raw));
    for word in &words {
        let digits: String = word.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if !digits.is_empty() && word.len() == digits.len() {
            if let Ok(number) = digits.parse::<u32>() {
                if number <= 999 {
                    return Some(number);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn episodes() -> Vec<(&'static str, &'static str, u32, u32)> {
        vec![
            ("furious.s01e01.1080p.cakes[EZTVx.to]", "Furious", 1, 1),
            ("furious.s01e04.1080p.cakes[EZTVx.to]", "Furious", 1, 4),
            ("Dark.Matter.2024.S02E05.1080p.DL.DDP5.1.Atmos.FLUX[TGx]", "Dark Matter", 2, 5),
            ("www.UIndex.org - Dark Matter S02E05 1080p WEBRip 10Bit DDP5 1 HEVC-d3g", "Dark Matter", 2, 5),
            ("Slow.Horses.S01E01.1080p.ATVP.WEB-DL.DDP5.1.H.264-MeM GP", "Slow Horses", 1, 1),
            ("The.Bureau.S01.10bit.x265", "The Bureau", 1, 0),
            ("The.Gentlemen.S01.DL.DDP5.1.ENG.TBK", "The Gentlemen", 1, 0),
            ("Wisting.S01.NORWEGiAN.HENRETTELSE[partly]", "Wisting", 1, 0),
            (
                "Escape at Dannemora (2018) Season 1 S01 (1080p AMZN WEB-DL x265 HEVC 10bit EAC3 5.1 t3nzin)",
                "Escape at Dannemora",
                1,
                0,
            ),
            ("City.of.Blood.S01.2160p.WEB-DL", "City of Blood", 1, 0),
            ("Last.Seen.S01E03.ITA.ENG.1080p.WEB-DL", "Last Seen", 1, 3),
            ("Show.1x02.HDTV.x264", "Show", 1, 2),
            ("Some.Series.Season 3.Episode 7.720p", "Some Series", 3, 7),
        ]
    }

    #[test]
    fn parses_episodes_from_real_release_names() {
        for (raw, title, season, episode) in episodes() {
            let parsed = parse(raw);
            assert_eq!(parsed.kind, NameKind::Episode, "nije epizoda: {raw}");
            assert_eq!(parsed.title, title, "naslov: {raw}");
            assert_eq!(parsed.season, Some(season), "sezona: {raw}");
            if episode > 0 {
                assert_eq!(parsed.episode, Some(episode), "epizoda: {raw}");
            }
        }
    }

    #[test]
    fn same_series_gets_same_key_although_names_differ() {
        let a = parse("Dark.Matter.2024.S02E05.1080p.DL.DDP5.1.Atmos.FLUX[TGx]");
        let b = parse("www.UIndex.org - Dark Matter S02E05 1080p WEBRip 10Bit DDP5 1 HEVC-d3g");
        assert_eq!(a.key, b.key);
        assert_eq!(a.key, "dark matter");
    }

    #[test]
    fn label_is_zero_padded_so_alphabetical_equals_numeric() {
        assert_eq!(parse("furious.s01e04.1080p.cakes").display(), "S01E04 · 1080p");
        assert_eq!(parse("Show.1x02.HDTV.x264").label(), "S01E02");
        assert_eq!(parse("The.Bureau.S01.10bit.x265").label(), "S01");
    }

    #[test]
    fn movies_keep_their_name_and_year() {
        let movie = parse("Last.Seen.2022.1080p.WEB-DL.DDP5.1.H.264-GROUP.mkv");
        assert_eq!(movie.kind, NameKind::Movie);
        assert_eq!(movie.title, "Last Seen");
        assert_eq!(movie.year, Some(2022));
        assert_eq!(movie.quality.as_deref(), Some("1080p"));
    }

    #[test]
    fn leading_tracker_junk_is_dropped() {
        let parsed = parse("www.UIndex.org - Slow Horses S01E02 1080p WEBRip");
        assert_eq!(parsed.title, "Slow Horses");
        assert_eq!(parsed.key, "slow horses");
    }

    #[test]
    fn lowercase_titles_are_capitalized_for_display() {
        assert_eq!(parse("slow.horses.s01e01.1080p").title, "Slow Horses");
        assert_eq!(parse("The.Bureau.S01.DL.DDP5.1.ENG.TBK").title, "The Bureau");
    }

    #[test]
    fn number_only_names_have_no_title_but_keep_the_episode() {
        let parsed = parse("S01E02.mkv");
        assert_eq!(parsed.title, "");
        assert_eq!(parsed.season, Some(1));
        assert_eq!(parsed.episode, Some(2));
        assert_eq!(parsed.label(), "S01E02");
    }

    #[test]
    fn episode_number_hint_reads_simple_numbers() {
        assert_eq!(episode_number_hint("03.mkv"), Some(3));
        assert_eq!(episode_number_hint("Epizoda 12.mkv"), Some(12));
        assert_eq!(episode_number_hint("film.mkv"), None);
    }

    #[test]
    fn junk_only_names_still_return_something() {
        let parsed = parse("1080p.mkv");
        assert_eq!(parsed.kind, NameKind::Movie);
        assert!(!parsed.title.is_empty());
    }
}
