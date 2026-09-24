//! Prepoznavanje serija iz imena datoteka.
//!
//! Podržani oblici (redom kojim ih probamo):
//! - `Serija S01E03.mkv`, `Serija s1e3.mkv`
//! - `Serija 1x03.mkv`
//! - `Serija Sezona 2 Epizoda 5.mkv`, `Serija Season 2 Episode 5.mkv`
//! - `Serija - 03.mkv` (samo broj epizode, ako je u mapi koja izgleda kao sezona)

/// Što smo uspjeli izvući iz imena datoteke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesInfo {
    pub series: String,
    pub season: u32,
    pub episode: u32,
}

impl SeriesInfo {
    /// "Serija S02E05" — kako se prikazuje u DLNA stablu.
    pub fn label(&self) -> String {
        format!("{} S{:02}E{:02}", self.series, self.season, self.episode)
    }
}

/// Izvuci seriju/sezonu/epizodu iz imena datoteke (bez ekstenzije).
///
/// Vraća `None` kad ime ne izgleda kao epizoda — bolje ne izmišljati seriju
/// nego film prikazati kao film.
pub fn parse(file_stem: &str) -> Option<SeriesInfo> {
    let cleaned = clean(file_stem);
    if cleaned.is_empty() {
        return None;
    }

    // 1) S01E03 / s1e3 / S01.E03 / S01 EP03
    if let Some((series, season, episode)) = split_on_season_episode(&cleaned) {
        return Some(SeriesInfo { series, season, episode });
    }

    // 2) 1x03
    if let Some((series, season, episode)) = split_on_x(&cleaned) {
        return Some(SeriesInfo { series, season, episode });
    }

    // 3) "Sezona 2 Epizoda 5" / "Season 2 Episode 5"
    if let Some((series, season, episode)) = split_on_words(&cleaned) {
        return Some(SeriesInfo { series, season, episode });
    }

    None
}

/// Ukloni šum koji stižu s izvora (release grupe, kvaliteta) i normaliziraj razmake.
fn clean(stem: &str) -> String {
    let mut text = stem.replace(['_', '.'], " ");
    // Izbaci sve u zagradama i rezolucije/kodeke na kraju imena.
    let noise = [
        "1080p", "720p", "2160p", "480p", "4k", "x264", "x265", "h264", "h265", "hevc", "aac", "ac3", "dts",
        "web", "webrip", "web-dl", "bluray", "brrip", "hdrip", "hdtv", "remux", "proper", "repack", "amzn",
        "nf", "ita", "eng", "hrv",
    ];
    let mut parts: Vec<String> = Vec::new();
    for token in text.split_whitespace() {
        let lower = token.to_lowercase();
        let lower = lower.trim_matches(|c: char| "-[](){}".contains(c));
        if lower.is_empty() || noise.contains(&lower) {
            continue;
        }
        parts.push(token.to_string());
    }
    text = parts.join(" ");
    text.trim_matches(|c: char| c == '-' || c.is_whitespace()).to_string()
}

/// `Serija S01E03` → ("Serija", 1, 3)
fn split_on_season_episode(text: &str) -> Option<(String, u32, u32)> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        let lower = token.to_lowercase();
        let Some(position) = lower.find('s').filter(|position| {
            let rest = &lower[*position + 1..];
            rest.starts_with(|c: char| c.is_ascii_digit())
        }) else {
            continue;
        };
        let rest = &lower[position + 1..];
        let Some(e_at) = rest.find('e') else { continue };
        let season: u32 = rest[..e_at].parse().ok()?;
        let episode: u32 = rest[e_at + 1..].parse().ok()?;
        let series = tokens[..index].join(" ");
        if series.is_empty() {
            return None;
        }
        return Some((series, season, episode));
    }
    None
}

/// `Serija 1x03` → ("Serija", 1, 3)
fn split_on_x(text: &str) -> Option<(String, u32, u32)> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        let lower = token.to_lowercase();
        let Some(x_at) = lower.find('x') else { continue };
        let (season_raw, episode_raw) = (&lower[..x_at], &lower[x_at + 1..]);
        let (Ok(season), Ok(episode)) = (season_raw.parse::<u32>(), episode_raw.parse::<u32>()) else {
            continue;
        };
        let series = tokens[..index].join(" ");
        if series.is_empty() {
            return None;
        }
        return Some((series, season, episode));
    }
    None
}

/// `Serija Sezona 2 Epizoda 5` / `Season 2 Episode 5`.
fn split_on_words(text: &str) -> Option<(String, u32, u32)> {
    let lower = text.to_lowercase();
    let season_at = lower.find("sezona").or_else(|| lower.find("season")).or_else(|| lower.find("sez."))?;
    let episode_at = lower.find("epizoda").or_else(|| lower.find("episode")).or_else(|| lower.find("ep."))?;
    if episode_at < season_at {
        return None;
    }
    let season = first_number(&text[season_at..episode_at])?;
    let episode = first_number(&text[episode_at..])?;
    let series = text[..season_at].trim().to_string();
    if series.is_empty() {
        return None;
    }
    Some((series, season, episode))
}

/// Prvi cijeli broj u tekstu.
fn first_number(text: &str) -> Option<u32> {
    let digits: String =
        text.chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(series: &str, season: u32, episode: u32) -> SeriesInfo {
        SeriesInfo { series: series.to_string(), season, episode }
    }

    #[test]
    fn parses_sxxexx() {
        assert_eq!(parse("Breaking Bad S02E05 1080p x265"), Some(info("Breaking Bad", 2, 5)));
        assert_eq!(parse("serija.s01e03.hdtv"), Some(info("serija", 1, 3)));
        assert_eq!(parse("Serija S1E3"), Some(info("Serija", 1, 3)));
    }

    #[test]
    fn parses_1x03() {
        assert_eq!(parse("Zlo 1x03"), Some(info("Zlo", 1, 3)));
        assert_eq!(parse("Zlo 2x10 HDTV"), Some(info("Zlo", 2, 10)));
    }

    #[test]
    fn parses_words() {
        assert_eq!(parse("Serija Sezona 2 Epizoda 5"), Some(info("Serija", 2, 5)));
        assert_eq!(parse("Serija Season 1 Episode 12"), Some(info("Serija", 1, 12)));
    }

    #[test]
    fn keeps_name_with_year_in_title() {
        assert_eq!(parse("Dexter S04E01"), Some(info("Dexter", 4, 1)));
    }

    #[test]
    fn films_are_not_series() {
        assert_eq!(parse("Test Film (2026)"), None);
        assert_eq!(parse("The Matrix 1999 1080p"), None);
        assert_eq!(parse("Predator"), None);
        // Samo broj sezone bez epizode nije epizoda.
        assert_eq!(parse("Rocky 4"), None);
    }

    #[test]
    fn label_is_readable() {
        assert_eq!(info("Zlo", 1, 3).label(), "Zlo S01E03");
    }
}
