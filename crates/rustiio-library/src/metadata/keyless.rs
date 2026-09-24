//! Izvori koji ne traže nikakav ključ.
//!
//! - **Wikipedia** — `pageimages` uz `pilicense=any` (bez toga posteri, koji su
//!   "non-free", uopće ne izlaze) daje poster filma s `upload.wikimedia.org`.
//! - **TVmaze** — službeni API za serije, poster do 2000×3000.
//! - **Cover Art Archive** — naslovnice albuma preko MusicBrainz ID-a.
//!
//! Provjereno živim pozivima: sva tri vraćaju `image/jpeg` bez ikakvog ključa.

use serde_json::Value;

use crate::metadata::{Found, Query, Source};

pub const USER_AGENT: &str = concat!("Rustiio/", env!("CARGO_PKG_VERSION"), " (media server; vaha.net)");

/// Wikipedia: poster iz članka (traži `pilicense=any`).
pub fn wikipedia_url(title: &str) -> String {
    format!(
        "https://en.wikipedia.org/w/api.php?action=query&prop=pageimages&format=json&pithumbsize=500&pilicense=any&titles={}",
        encode(title)
    )
}

/// Wikipedia: pretraga koja odmah vraća i slike (jedan zahtjev, bez dva kruga).
///
/// `generator=search` rješava i dijakritiku i dodatke u naslovu: upit `Amelie`
/// nađe članak `Amélie`, a `Sicario` nađe `Sicario (2015 film)`.
pub fn wikipedia_search_images_url(title: &str) -> String {
    format!(
        "https://en.wikipedia.org/w/api.php?action=query&generator=search&gsrlimit=4&prop=pageimages&pithumbsize=500&pilicense=any&format=json&gsrsearch={}",
        encode(title)
    )
}

/// Wikipedia: pronađi članak ako točan naslov ne postoji.
pub fn wikipedia_search_url(title: &str) -> String {
    format!(
        "https://en.wikipedia.org/w/api.php?action=query&list=search&format=json&srlimit=1&srsearch={}",
        encode(title)
    )
}

/// TVmaze: pretraga serije.
pub fn tvmaze_url(title: &str) -> String {
    format!("https://api.tvmaze.com/search/shows?q={}", encode(title))
}

/// MusicBrainz: pretraga albuma (Cover Art Archive traži MBID).
pub fn musicbrainz_url(album: &str) -> String {
    format!("https://musicbrainz.org/ws/2/release-group/?fmt=json&limit=1&query={}", encode(album))
}

/// Cover Art Archive: naslovnica albuma (bez ključa).
pub fn coverart_url(mbid: &str) -> String {
    format!("https://coverartarchive.org/release-group/{mbid}/front-500")
}

fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => encoded.push(byte as char),
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Izvuci URL slike iz Wikipedia `pageimages` odgovora.
pub fn wikipedia_image(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    value
        .get("query")?
        .get("pages")?
        .as_object()?
        .values()
        .find_map(|page| page.get("thumbnail")?.get("source")?.as_str().map(|url| url.to_string()))
}

/// Prvi naslov iz Wikipedia pretrage.
pub fn wikipedia_first_title(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    value
        .get("query")?
        .get("search")?
        .as_array()?
        .first()?
        .get("title")?
        .as_str()
        .map(|title| title.to_string())
}

/// Poster iz TVmaze odgovora (najkvalitetnija slika prvog pogotka).
pub fn tvmaze_image(json: &str) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(json).ok()?;
    let first = value.as_array()?.first()?;
    let show = first.get("show")?;
    let name = show.get("name")?.as_str()?.to_string();
    let url = show.get("image")?.get("original")?.as_str()?.to_string();
    Some((name, url))
}

/// MusicBrainz ID-evi prvih nekoliko albuma (prvi pogodak nema uvijek naslovnicu).
pub fn musicbrainz_ids(json: &str, limit: usize) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    value
        .get("release-groups")
        .and_then(|groups| groups.as_array())
        .map(|groups| {
            groups
                .iter()
                .take(limit)
                .filter_map(|group| group.get("id").and_then(|id| id.as_str()).map(|id| id.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// MusicBrainz ID prvog albuma.
pub fn musicbrainz_first_id(json: &str) -> Option<String> {
    musicbrainz_ids(json, 1).into_iter().next()
}

/// Naslov i slika svake stranice iz `generator=search` odgovora (redoslijed po relevantnosti).
pub fn wikipedia_pages(json: &str) -> Vec<(String, Option<String>)> {
    let Ok(value) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let Some(pages) =
        value.get("query").and_then(|query| query.get("pages")).and_then(|pages| pages.as_object())
    else {
        return Vec::new();
    };
    let mut entries: Vec<(i64, String, Option<String>)> = pages
        .values()
        .map(|page| {
            let title = page.get("title").and_then(|title| title.as_str()).unwrap_or_default().to_string();
            let image = page
                .get("thumbnail")
                .and_then(|thumb| thumb.get("source"))
                .and_then(|source| source.as_str())
                .map(|url| url.to_string());
            let index = page.get("index").and_then(|index| index.as_i64()).unwrap_or(i64::MAX);
            (index, title, image)
        })
        .collect();
    entries.sort_by_key(|(index, _, _)| *index);
    entries.into_iter().map(|(_, title, image)| (title, image)).collect()
}

/// Liči li naslov članka na ono što tražimo (`Sicario (2015 film)` → `sicario`).
fn title_matches(article: &str, wanted: &str) -> bool {
    let base = article.split(" (").next().unwrap_or(article);
    let base = super::tmdb::normalize_title(base);
    base == wanted || (!wanted.is_empty() && (base.starts_with(wanted) || wanted.starts_with(&base)))
}

/// Wikipedia poster: točan naslov, pa pretraga s generiranim slikama.
pub fn wikipedia_poster(agent: &ureq::Agent, query: &Query) -> Option<Found> {
    // 1. Točan naslov je najprecizniji: `Sicario (2015 film)`, `Amélie (film)`.
    let mut exact = vec![query.title.clone()];
    if let Some(year) = query.year.filter(|_| !query.is_series) {
        exact.insert(0, format!("{} ({year} film)", query.title));
        exact.insert(1, format!("{} (film)", query.title));
    }
    for title in exact {
        if let Some(json) = get_text(agent, &wikipedia_url(&title)) {
            if let Some(image) = wikipedia_image(&json) {
                return download(agent, Source::Wikipedia, &image);
            }
        }
    }

    // 2. `generator=search` u jednom zahtjevu vraća i članke i slike — rješava
    //    dijakritiku (`Amelie` → `Amélie`) i dodatke u naslovu.
    let json = get_text(agent, &wikipedia_search_images_url(&query.title))?;
    let pages = wikipedia_pages(&json);
    let wanted = super::tmdb::normalize_title(&query.title);
    let picked = pages
        .iter()
        .find(|(title, image)| image.is_some() && title_matches(title, &wanted))
        .or_else(|| pages.iter().find(|(_, image)| image.is_some()))?;
    download(agent, Source::Wikipedia, picked.1.as_deref()?)
}

/// TVmaze poster (samo serije).
pub fn tvmaze_poster(agent: &ureq::Agent, query: &Query) -> Option<Found> {
    if !query.is_series {
        return None;
    }
    let json = get_text(agent, &tvmaze_url(&query.title))?;
    let (name, url) = tvmaze_image(&json)?;
    // Naslov iz TVmaza mora ličiti na ono što tražimo, inače je pogrešna serija.
    if !similar(&name, &query.title) {
        return None;
    }
    let bytes = super::tmdb::fetch_bytes(agent, &url)?;
    let extension = super::tmdb::extension_for(&bytes)?;
    Some(Found { source: Source::Tvmaze, url, bytes, extension })
}

/// Cover Art Archive naslovnica (glazba).
///
/// MusicBrainz često vrati album bez naslovnice (singl, kompilacija), pa se
/// pokušava nekoliko pogodaka — prvi s naslovnicom pobjeđuje.
pub fn coverart_poster(agent: &ureq::Agent, query: &Query) -> Option<Found> {
    let json = get_text(agent, &musicbrainz_url(&query.title))?;
    for mbid in musicbrainz_ids(&json, COVERART_ATTEMPTS) {
        if let Some(found) = download(agent, Source::CoverArt, &coverart_url(&mbid)) {
            return Some(found);
        }
    }
    None
}

/// Koliko MusicBrainz pogodaka provjeriti prije nego odustanemo.
pub const COVERART_ATTEMPTS: usize = 3;

/// Dohvati sliku i provjeri da je stvarno slika (a ne HTML s greškom).
fn download(agent: &ureq::Agent, source: Source, url: &str) -> Option<Found> {
    let bytes = super::tmdb::fetch_bytes(agent, url)?;
    let extension = super::tmdb::extension_for(&bytes)?;
    Some(Found { source, url: url.to_string(), bytes, extension })
}

/// GET koji vraća tijelo kao tekst (uz naš User-Agent — MusicBrainz ga zahtijeva).
///
/// Ponavlja prolazne greške: MusicBrainz dopušta 1 zahtjev u sekundi i vraća 503
/// kad se prekorači, a Cover Art Archive zna preusmjeriti na arhivu koja zastane.
fn get_text(agent: &ureq::Agent, url: &str) -> Option<String> {
    for attempt in 0..super::tmdb::RETRIES {
        super::pacing::global().wait(url);
        if attempt > 0 {
            std::thread::sleep(super::tmdb::backoff(attempt));
        }
        match agent.get(url).header("User-Agent", USER_AGENT).call() {
            Ok(mut response) => {
                // I prekinut prijenos tijela je prolazna greška (Wikipedia zna zastati).
                if let Ok(body) = response.body_mut().read_to_string() {
                    return Some(body);
                }
            }
            Err(error) if super::tmdb::retryable(&error) => continue,
            Err(_) => return None,
        }
    }
    None
}

/// Gruba usporedba naslova (mala/velika slova, interpunkcija).
fn similar(left: &str, right: &str) -> bool {
    let normalize = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let (a, b) = (normalize(left), normalize(right));
    a == b || a.starts_with(&b) || b.starts_with(&a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikipedia_needs_pilicense_any() {
        let url = wikipedia_url("Sicario (2015 film)");
        assert!(url.contains("pilicense=any"), "bez ovoga posteri ne izlaze");
        assert!(url.contains("titles=Sicario+%282015+film%29"));
    }

    #[test]
    fn reads_wikipedia_thumbnail() {
        let json = r#"{"query":{"pages":{"123":{"pageid":123,"title":"Sicario (2015 film)",
            "thumbnail":{"source":"https://upload.wikimedia.org/wikipedia/en/4/4b/Sicario_poster.jpg","width":260,"height":384}}}}}"#;
        assert_eq!(
            wikipedia_image(json).as_deref(),
            Some("https://upload.wikimedia.org/wikipedia/en/4/4b/Sicario_poster.jpg")
        );
        assert!(wikipedia_image(r#"{"query":{"pages":{}}}"#).is_none());
    }

    #[test]
    fn reads_wikipedia_search_title() {
        let json = r#"{"query":{"search":[{"title":"Sicario (2015 film)"},{"title":"Sicario"}]}}"#;
        assert_eq!(wikipedia_first_title(json).as_deref(), Some("Sicario (2015 film)"));
    }

    #[test]
    fn reads_tvmaze_poster() {
        let json = r#"[{"score":9.0,"show":{"name":"Dark Matter","image":{"medium":"m.jpg","original":"https://static.tvmaze.com/uploads/images/original_untouched/633/1584721.jpg"}}}]"#;
        let (name, url) = tvmaze_image(json).expect("serija");
        assert_eq!(name, "Dark Matter");
        assert!(url.ends_with("1584721.jpg"));
        assert!(tvmaze_image("[]").is_none());
    }

    #[test]
    fn reads_musicbrainz_id_and_builds_coverart_url() {
        let json =
            r#"{"release-groups":[{"id":"1b022e01-4da6-387b-8658-8678046e4cef","title":"Nevermind"}]}"#;
        let id = musicbrainz_first_id(json).expect("mbid");
        assert_eq!(musicbrainz_ids(json, 3).len(), 1, "poštuje limit i oblik odgovora");
        assert_eq!(musicbrainz_ids(json, 0).len(), 0);
        assert_eq!(
            coverart_url(&id),
            "https://coverartarchive.org/release-group/1b022e01-4da6-387b-8658-8678046e4cef/front-500"
        );
        assert!(musicbrainz_first_id(r#"{"release-groups":[]}"#).is_none());
    }

    #[test]
    fn similar_ignores_case_and_punctuation() {
        assert!(similar("Dark Matter", "dark matter"));
        assert!(similar("Sicario: Day of the Soldado", "sicario day of the soldado"));
        assert!(!similar("Dark Matter", "Family Matters"));
        assert!(!similar("Dark Matter", "Matter Dark"));
    }

    #[test]
    fn user_agent_is_set_for_musicbrainz() {
        assert!(USER_AGENT.starts_with("Rustiio/"));
    }
}
