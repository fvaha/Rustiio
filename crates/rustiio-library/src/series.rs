//! Serije iz imena datoteka — tanki sloj nad [`crate::naming`].
//!
//! Parser je jedan za cijeli projekt (i katalog i bazu i web); ovdje ostaje samo
//! oblik koji traži baza (`items.series`) i oznaka za prikaz.

use crate::naming::{self, NameKind};

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
/// Vraća `None` kad ime ne izgleda kao epizoda ili kad nema naziva serije —
/// bolje ne izmišljati seriju nego film prikazati kao film.
pub fn parse(file_stem: &str) -> Option<SeriesInfo> {
    let parsed = naming::parse(file_stem);
    if parsed.kind != NameKind::Episode || parsed.title.is_empty() {
        return None;
    }
    let (Some(season), Some(episode)) = (parsed.season, parsed.episode) else {
        return None; // samo sezona (komplet) nije epizoda
    };
    Some(SeriesInfo { series: parsed.title, season, episode })
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
        assert_eq!(parse("serija.s01e03.hdtv"), Some(info("Serija", 1, 3)));
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
        assert_eq!(parse("The.Bureau.S01.10bit.x265"), None, "komplet sezone nije epizoda");
    }

    #[test]
    fn label_is_readable() {
        assert_eq!(info("Zlo", 1, 3).label(), "Zlo S01E03");
    }
}
