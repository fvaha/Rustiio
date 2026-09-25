//! TMDB: s ključem preko API-ja, bez ključa preko web stranice.
//!
//! Ključ (API v3 `?api_key=` ili v4 token kao `Authorization: Bearer`) daje stabilan
//! JSON. Bez ključa se čita javna stranica pretrage i iz nje izvuče put postera —
//! slike same (`media.themoviedb.org`) ne traže ključ. Web put je krhkiji: ako se
//! HTML promijeni, prazan rezultat je jedina posljedica (red izvora ide dalje).

use crate::metadata::{Found, Query, Source};

pub const API_BASE: &str = "https://api.themoviedb.org/3";
pub const IMAGE_BASE: &str = "https://media.themoviedb.org/t/p";
pub const WEB_BASE: &str = "https://www.themoviedb.org";
pub const DEFAULT_WIDTH: &str = "w500";

/// URL slike iz `hash`-a (`lz8vNyXeidqqOdJW9ZjnDAMb5Vr.jpg`).
pub fn image_url(hash: &str, width: &str) -> String {
    format!("{IMAGE_BASE}/{width}/{hash}")
}

/// URL pretrage na API-ju (v3 ključ).
pub fn api_search_url(kind: &str, query: &str, key: &str) -> String {
    format!("{API_BASE}/search/{kind}?query={}&api_key={key}", encode(query))
}

/// URL pretrage na web stranici (bez ključa).
pub fn web_search_url(kind: &str, query: &str) -> String {
    format!("{WEB_BASE}/search/{kind}?query={}", encode(query))
}

/// Procjena je li vrijednost v4 token (`eyJ...`) a ne v3 ključ.
pub fn looks_like_v4_token(value: &str) -> bool {
    value.starts_with("eyJ") && value.len() > 100
}

/// Rezultat pretrage: naslov, godina i put do postera.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub id: String,
    pub title: String,
    pub year: Option<u32>,
    pub poster_hash: Option<String>,
}

/// `?query=` vrijednost bez ovisnosti o vanjskoj biblioteci.
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

/// Izvuci rezultate iz HTML-a pretrage na TMDB-u.
///
/// Kartica izgleda ovako:
/// `<a data-media-type="movie" href="/movie/273481-sicario">…<img class="poster" srcset="https://media.themoviedb.org/t/p/w94_and_h141_face/lz8vNyXeidqqOdJW9ZjnDAMb5Vr.jpg 1x, …">`
///
/// Svi rezovi idu po ASCII graničnicima (znamenke, `"`, `-`), pa stranica s
/// nelatiničnim znakovima (TMDB poslužuje i ćirilicu) ne može srušiti parser.
pub fn parse_web_results(html: &str, kind: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let prefix = format!("/{kind}/");

    // Kartice su odvojene atributom `data-object-id="` — to je i granica pretrage
    // postera, da se ne uzme slika iz susjedne kartice.
    for chunk in html.split("data-object-id=\"").skip(1) {
        let Some(found) = chunk.find(&prefix) else { continue };
        let after_prefix = found + prefix.len();

        let digits = ascii_run(&chunk[after_prefix..], |byte| byte.is_ascii_digit());
        if digits.is_empty() {
            continue;
        }
        let id = digits.to_string();

        // `/movie/273481-sicario` → naslov iz sluga.
        let mut title = String::new();
        let tail = &chunk[after_prefix + digits.len()..];
        if let Some(slug) = tail.strip_prefix('-') {
            let slug = ascii_run(slug, |byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
            title = slug.replace('-', " ");
        }
        // `alt="..."` je čitljiviji (i lokaliziran) naslov.
        if let Some(alt_start) = chunk.find("alt=\"") {
            let rest = &chunk[alt_start + "alt=\"".len()..];
            let alt = ascii_run(rest, |byte| byte != b'"');
            if !alt.trim().is_empty() {
                title = alt.to_string();
            }
        }

        let poster_hash = chunk.find("/t/p/w94_and_h141_face/").map(|at| {
            let rest = &chunk[at + "/t/p/w94_and_h141_face/".len()..];
            ascii_run(rest, |byte| {
                byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_' || byte == b'-'
            })
            .to_string()
        });
        let poster_hash = poster_hash.filter(|hash| !hash.is_empty());

        // Ista kartica se pojavi dvaput (desktop/mobilna) — dedup po id-u.
        if !hits.iter().any(|existing: &Hit| existing.id == id) {
            hits.push(Hit { id, title, year: None, poster_hash });
        }
    }
    hits
}

/// Uzmi početni niz bajtova koji prolaze filtar (rez je uvijek na ASCII granici).
fn ascii_run(text: &str, keep: impl Fn(u8) -> bool) -> &str {
    let length = text.as_bytes().iter().take_while(|byte| keep(**byte)).count();
    // Filtar propušta samo ASCII bajtove, pa je `length` granica znaka.
    text.get(..length).unwrap_or("")
}

/// Naslov iz `alt`-a je često lokaliziran pa se ne poklapa s upitom.
pub(crate) fn normalize_title(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Izvuci rezultate iz API JSON-a (v3/v4).
pub fn parse_api_results(json: &str) -> Vec<Hit> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    value
        .get("results")
        .and_then(|results| results.as_array())
        .map(|results| {
            results
                .iter()
                .map(|entry| {
                    let title = entry
                        .get("title")
                        .or_else(|| entry.get("name"))
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let date = entry
                        .get("release_date")
                        .or_else(|| entry.get("first_air_date"))
                        .and_then(|value| value.as_str())
                        .unwrap_or_default();
                    Hit {
                        id: entry.get("id").map(|id| id.to_string()).unwrap_or_default(),
                        title,
                        year: date.get(..4).and_then(|year| year.parse().ok()),
                        poster_hash: entry
                            .get("poster_path")
                            .and_then(|path| path.as_str())
                            .map(|path| path.trim_start_matches('/').to_string()),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Odaberi najbolji pogodak: prvo točan naslov, pa godina, inače prvi s posterom.
pub fn best_hit(hits: &[Hit], query: &Query) -> Option<Hit> {
    // Interpunkcija i velika slova se ignoriraju: `Sicario: Day of the Soldado`
    // mora se prepoznati i kad upit dođe iz imena datoteke bez dvotočke.
    let wanted = normalize_title(&query.title);
    let mut candidates: Vec<&Hit> = hits.iter().filter(|hit| hit.poster_hash.is_some()).collect();
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by_key(|hit| {
        let title = normalize_title(&hit.title);
        let exact = title == wanted;
        let starts = title.starts_with(&wanted) || wanted.starts_with(&title);
        let year_matches = query.year.is_some() && hit.year == query.year;
        // Niže je bolje.
        match (exact, year_matches, starts) {
            (true, true, _) => 0,
            (true, false, _) => 1,
            (false, true, _) => 2,
            (false, false, true) => 3,
            _ => 4,
        }
    });
    candidates.first().map(|hit| (*hit).clone())
}

/// Poster iz TMDB-a (API ako imamo ključ, inače web).
pub fn poster(agent: &ureq::Agent, query: &Query, api_key: Option<&str>, width: &str) -> Option<Found> {
    let kind = if query.is_series { "tv" } else { "movie" };
    let (hits, source) = match api_key {
        Some(key) => (fetch_api(agent, kind, &query.title, key).unwrap_or_default(), Source::Tmdb),
        None => (fetch_web(agent, kind, &query.title).unwrap_or_default(), Source::TmdbWeb),
    };

    let hit = best_hit(&hits, query)?;
    let hash = hit.poster_hash?;
    let url = image_url(&hash, width);
    let bytes = fetch_bytes(agent, &url)?;
    let extension = extension_for(&bytes)?;
    Some(Found { source, url, bytes, extension })
}

/// JSON s API-ja (v3 ključ u upitu, v4 token u zaglavlju).
fn fetch_api(agent: &ureq::Agent, kind: &str, title: &str, key: &str) -> Option<Vec<Hit>> {
    let request = if looks_like_v4_token(key) {
        agent
            .get(&format!("{API_BASE}/search/{kind}?query={}", encode(title)))
            .header("Authorization", &format!("Bearer {key}"))
    } else {
        agent.get(&api_search_url(kind, title, key))
    };
    let mut response = request.call().ok()?;
    let body = response.body_mut().read_to_string().ok()?;
    let hits = parse_api_results(&body);
    (!hits.is_empty()).then_some(hits)
}

/// HTML s javne stranice pretrage.
fn fetch_web(agent: &ureq::Agent, kind: &str, title: &str) -> Option<Vec<Hit>> {
    let mut response = agent
        .get(&web_search_url(kind, title))
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Accept", "text/html,application/xhtml+xml")
        .call()
        .ok()?;
    let body = response.body_mut().read_to_string().ok()?;
    let hits = parse_web_results(&body, kind);
    (!hits.is_empty()).then_some(hits)
}

/// Dohvati sliku (i provjeri da je slika, ne HTML s greškom).
pub fn fetch_bytes(agent: &ureq::Agent, url: &str) -> Option<Vec<u8>> {
    for attempt in 0..RETRIES {
        super::pacing::global().wait(url);
        if attempt > 0 {
            std::thread::sleep(backoff(attempt));
        }
        let result = agent.get(url).call();
        match result {
            Ok(mut response) => {
                // Prekinut prijenos je prolazna greška — probaj ponovno.
                let Ok(bytes) = response.body_mut().read_to_vec() else { continue };
                if bytes.len() > 1024 {
                    return Some(bytes);
                }
                // Premalo tijelo nije prolazna greška (npr. 404 stranica).
                return None;
            }
            // Javni API-ji redovno vraćaju 429/5xx (MusicBrainz ograničava na 1 zahtjev/s).
            Err(error) if retryable(&error) => continue,
            Err(_) => return None,
        }
    }
    None
}

/// Koliko puta ponoviti prolaznu grešku.
pub const RETRIES: usize = 3;

/// Koliko čekati prije ponovnog pokušaja (rate-limit traži dulje od mrežne greške).
pub fn backoff(attempt: usize) -> std::time::Duration {
    match attempt {
        0 => std::time::Duration::ZERO,
        1 => std::time::Duration::from_millis(800),
        2 => std::time::Duration::from_millis(2500),
        _ => std::time::Duration::from_millis(6000),
    }
}

/// Vrijedi li pokušati ponovno (mreža ili poslužitelj privremeno nedostupan).
pub fn retryable(error: &ureq::Error) -> bool {
    match error {
        ureq::Error::StatusCode(code) => *code == 429 || (500..=599).contains(code),
        _ => true,
    }
}

/// Ekstenzija po sadržaju (TMDB servira JPEG, ponekad PNG).
pub fn extension_for(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML: &str = r#"
      <div data-object-id="538ca2990e0a2667150046ae"><div class="flex flex-nowrap">
        <a class="flex w-full" data-media-type="movie" href="/movie/273481-sicario">
          <div class="image"><img alt="Sicario" class="poster" srcset="https://media.themoviedb.org/t/p/w94_and_h141_face/lz8vNyXeidqqOdJW9ZjnDAMb5Vr.jpg 1x, https://media.themoviedb.org/t/p/w188_and_h282_face/lz8vNyXeidqqOdJW9ZjnDAMb5Vr.jpg 2x" src="https://media.themoviedb.org/t/p/w94_and_h141_face/lz8vNyXeidqqOdJW9ZjnDAMb5Vr.jpg"></div>
        </a></div></div>
      <div data-object-id="abc"><div class="flex flex-nowrap">
        <a class="flex w-full" data-media-type="movie" href="/movie/400535-sicario-day-of-the-soldado">
          <div class="image"><img alt="Sicario: Day of the Soldado" class="poster" srcset="https://media.themoviedb.org/t/p/w94_and_h141_face/qcLYofEhNh51Sk1jUWjmKHLzkqw.jpg 1x"></div>
        </a></div></div>
    "#;

    #[test]
    fn parses_web_search_html() {
        let hits = parse_web_results(HTML, "movie");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, "273481");
        // Naslov se čita iz `alt`-a (čitljiviji od sluga).
        assert_eq!(hits[0].title, "Sicario");
        assert_eq!(hits[0].poster_hash.as_deref(), Some("lz8vNyXeidqqOdJW9ZjnDAMb5Vr.jpg"));
        assert_eq!(hits[1].id, "400535");
        assert_eq!(hits[1].title, "Sicario: Day of the Soldado");
    }

    #[test]
    fn parser_survives_non_latin_pages() {
        // TMDB poslužuje i ćirilicu; rezovi moraju ići po ASCII granicama.
        let cyrillic = r#"<div data-object-id="x"><a href="/movie/273481-sicario"><img alt="Сикарио" class="poster" srcset="https://media.themoviedb.org/t/p/w94_and_h141_face/lz8vNyXeidqqOdJW9ZjnDAMb5Vr.jpg 1x"></div>"#;
        let hits = parse_web_results(cyrillic, "movie");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "273481");
        assert_eq!(hits[0].poster_hash.as_deref(), Some("lz8vNyXeidqqOdJW9ZjnDAMb5Vr.jpg"));

        // Pokvaren HTML ne smije srušiti parser.
        assert!(parse_web_results("<div data-object-id=\"x\"><a href=\"/movie/\">", "movie").is_empty());
        assert!(parse_web_results("", "movie").is_empty());
    }

    #[test]
    fn picks_exact_title_and_year() {
        let hits = parse_web_results(HTML, "movie");
        let query = Query { title: "Sicario".to_string(), year: Some(2015), is_series: false };
        let best = best_hit(&hits, &query).expect("pogodak");
        assert_eq!(best.id, "273481");

        let sequel = Query { title: "sicario day of the soldado".to_string(), year: None, is_series: false };
        assert_eq!(best_hit(&hits, &sequel).expect("pogodak").id, "400535");
    }

    #[test]
    fn parses_api_json() {
        let json = r#"{"results":[
            {"id":273481,"title":"Sicario","release_date":"2015-09-17","poster_path":"/lz8vNyXeidqqOdJW9ZjnDAMb5Vr.jpg"},
            {"id":1,"name":"Dark Matter","first_air_date":"2024-05-08","poster_path":null}
        ]}"#;
        let hits = parse_api_results(json);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].year, Some(2015));
        assert_eq!(hits[0].poster_hash.as_deref(), Some("lz8vNyXeidqqOdJW9ZjnDAMb5Vr.jpg"));
        assert_eq!(hits[1].title, "Dark Matter");
        assert!(hits[1].poster_hash.is_none());

        // Bez postera nema pogotka: traži se serija koja postoji samo bez slike.
        let posterless =
            parse_api_results(r#"{"results":[{"id":1,"name":"Dark Matter","poster_path":null}]}"#);
        let query = Query { title: "Dark Matter".to_string(), year: None, is_series: true };
        assert!(best_hit(&posterless, &query).is_none());

        // Prazan/pokvaren JSON nije greška, samo nema rezultata.
        assert!(parse_api_results("").is_empty());
        assert!(parse_api_results(r#"{"status_code":7}"#).is_empty());
    }

    #[test]
    fn urls_are_keyless_for_images_and_encoded_for_search() {
        assert_eq!(image_url("abc.jpg", "w500"), "https://media.themoviedb.org/t/p/w500/abc.jpg");
        assert_eq!(
            web_search_url("movie", "zestoki decki"),
            "https://www.themoviedb.org/search/movie?query=zestoki+decki"
        );
        assert_eq!(
            api_search_url("tv", "dark matter", "KEY"),
            "https://api.themoviedb.org/3/search/tv?query=dark+matter&api_key=KEY"
        );
    }

    #[test]
    fn detects_v4_tokens() {
        assert!(!looks_like_v4_token("0123456789abcdef0123456789abcdef"));
        assert!(looks_like_v4_token(&format!("eyJ{}", "a".repeat(120))));
    }
}

/// Zapis s TMDB-a za **točno jedan** ID (`/tv/{id}` ili `/movie/{id}`).
#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub title: String,
    pub year: Option<u32>,
    pub poster_hash: Option<String>,
}

/// URL zapisa po ID-u (v3 ključ u upitu; v4 token ide u zaglavlje).
pub fn detail_url(kind: &str, id: &str, key: &str) -> String {
    format!("{API_BASE}/{kind}/{id}?api_key={key}&language=en-US")
}

/// Ista polja kao pretraga: `name`/`first_air_date` za serije, `title`/`release_date` za filmove.
pub fn parse_detail(json: &str) -> Option<Detail> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let title =
        value.get("title").or_else(|| value.get("name")).and_then(|field| field.as_str())?.to_string();
    let date = value
        .get("release_date")
        .or_else(|| value.get("first_air_date"))
        .and_then(|field| field.as_str())
        .unwrap_or_default();
    Some(Detail {
        title,
        year: date.get(..4).and_then(|year| year.parse().ok()),
        poster_hash: value
            .get("poster_path")
            .and_then(|path| path.as_str())
            .map(|path| path.trim_start_matches('/').to_string()),
    })
}

/// Isto, ali s javne stranice (`og:title`, `og:image`) kad nema ključa.
pub fn parse_web_detail(html: &str) -> Option<Detail> {
    let polje = |ime: &str| -> Option<String> {
        let uzorak = format!("property=\"og:{ime}\"");
        let start = html.find(&uzorak)?;
        let ostatak = &html[start..];
        let vrijednost = ostatak.find("content=\"")? + "content=\"".len();
        let kraj = ostatak[vrijednost..].find('"')?;
        Some(ostatak[vrijednost..vrijednost + kraj].to_string())
    };
    let naslov = polje("title")?;
    // `Slow Horses (TV Series 2022- ) | TMDB` → naslov i godina.
    let cist = naslov.split(" | ").next().unwrap_or(&naslov);
    let title = cist.split(" (").next().unwrap_or(cist).trim().to_string();
    // Unutar zagrada je `TV Series 2024- ` ili `2015-09-17`: uzmi prvi četveroznamenkasti broj.
    let year = cist
        .split(" (")
        .nth(1)
        .and_then(|rep: &str| {
            rep.split(|znak: char| !znak.is_ascii_digit() && znak != '-').find(|token| token.len() >= 4)
        })
        .and_then(|token| token.get(..4))
        .and_then(|year| year.parse().ok());
    // `…/t/p/w300_and_h450_bestv2/<hash>.jpg` → hash.
    let slika = polje("image")?;
    let datoteka = slika.rsplit('/').next()?;
    let hash = datoteka.rsplit_once('.').map(|(hash, _)| hash).unwrap_or(datoteka);
    Some(Detail { title, year, poster_hash: Some(hash.to_string()) })
}

/// Najbolje ocijenjen plakat iz galerije zapisa.
///
/// `poster_path` na zapisu je često tuđa slika (glazbeni omot, zastava, fan
/// uradak s nula glasova), pa se bira plakat s najviše glasova — i vraća se i
/// broj glasova da pozivatelj zna je li dobio nešto što je itko potvrdio.
pub fn gallery_best_poster(
    agent: &ureq::Agent,
    kind: &str,
    id: &str,
    api_key: Option<&str>,
) -> Option<(String, u32)> {
    let key = api_key?;
    let url = format!("{API_BASE}/{kind}/{id}/images?api_key={key}&include_image_language=en,null");
    let mut response = agent.get(&url).call().ok()?;
    let body = response.body_mut().read_to_string().ok()?;
    let value: serde_json::Value = serde_json::from_str(&body).ok()?;
    let posteri = value.get("posters")?.as_array()?;
    let mut najbolji: Option<(String, u32, f64)> = None;
    for poster in posteri {
        let Some(putanja) = poster.get("file_path").and_then(|p| p.as_str()) else {
            continue;
        };
        let jezik = poster.get("iso_639_1").and_then(|j| j.as_str()).unwrap_or("");
        if !jezik.is_empty() && jezik != "en" {
            continue;
        }
        let glasova = poster.get("vote_count").and_then(|g| g.as_u64()).unwrap_or(0) as u32;
        let ocjena = poster.get("vote_average").and_then(|o| o.as_f64()).unwrap_or(0.0);
        let bolji = match &najbolji {
            None => true,
            Some((_, najglasova, najocjena)) => {
                glasova > *najglasova || (glasova == *najglasova && ocjena > *najocjena)
            }
        };
        if bolji {
            najbolji = Some((putanja.trim_start_matches('/').to_string(), glasova, ocjena));
        }
    }
    najbolji.map(|(hash, glasova, _)| (hash, glasova))
}

/// Dohvati zapis po ID-u (API ako ima ključ, inače javna stranica).
pub fn detail(agent: &ureq::Agent, kind: &str, id: &str, api_key: Option<&str>) -> Option<Detail> {
    let (url, body) = match api_key {
        Some(key) => {
            let url = detail_url(kind, id, key);
            let mut response = agent.get(&url).call().ok()?;
            (url, response.body_mut().read_to_string().ok()?)
        }
        None => {
            let url = format!("{WEB_BASE}/{kind}/{id}");
            let mut response = agent
                .get(&url)
                .header("Accept-Language", "en-US,en;q=0.9")
                .header("Accept", "text/html,application/xhtml+xml")
                .call()
                .ok()?;
            let html = response.body_mut().read_to_string().ok()?;
            return parse_web_detail(&html);
        }
    };
    let _ = url;
    parse_detail(&body)
}

/// Poster **po ID-u**: nema pretrage ni pogađanja naslova — točno taj zapis.
pub fn poster_by_id(
    agent: &ureq::Agent,
    kind: &str,
    id: &str,
    api_key: Option<&str>,
    width: &str,
) -> Option<Found> {
    // Galerija ima plakate s glasovima zajednice; `poster_path` je često
    // nečija tuđa slika (glazbeni omot, zastava, fan uradak bez glasova).
    if let Some((hash, glasova)) = gallery_best_poster(agent, kind, id, api_key) {
        if glasova >= 1
            && let Some(found) = image_as_found(agent, &hash, width, Source::Tmdb)
        {
            tracing::info!(kind, id, glasova, "plakat iz galerije zapisa");
            return Some(found);
        }
    }
    let zapis = detail(agent, kind, id, api_key)?;
    let hash = zapis.poster_hash?;
    let source = if api_key.is_some() { Source::Tmdb } else { Source::TmdbWeb };
    image_as_found(agent, &hash, width, source)
}

/// Slika s TMDB-a kao `Found` (uz provjeru da je stvarno slika).
fn image_as_found(agent: &ureq::Agent, hash: &str, width: &str, source: Source) -> Option<Found> {
    let url = image_url(hash, width);
    let bytes = fetch_bytes(agent, &url)?;
    let extension = extension_for(&bytes)?;
    Some(Found { source, url, bytes, extension })
}

/// Smije li se ID zapamtiti: naslov se mora slagati, a labavo poklapanje traži i godinu.
pub fn titles_match(wanted: &str, found: &str, wanted_year: Option<u32>, found_year: Option<u32>) -> bool {
    // Godina ima zadnju riječ: ako se obje znaju i daleko su, to je drugi naslov
    // (naš `Fall 2` iz 2026. nije isto što i zapis iz 1970.).
    if let (Some(trazena), Some(nadena)) = (wanted_year, found_year)
        && trazena.abs_diff(nadena) > 2
    {
        return false;
    }

    let a = normalize_title(&bez_godine(wanted));
    let b = normalize_title(&bez_godine(found));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    let year_ok = matches!((wanted_year, found_year), (Some(w), Some(f)) if w.abs_diff(f) <= 1);
    year_ok && (a.starts_with(&b) || b.starts_with(&a))
}

/// `The Fix (2026)` i `The Fix 2026` su isti naslov kao `The Fix` — godina se
/// uspoređuje zasebno, pa ne smije ulaziti u poklapanje imena.
fn bez_godine(text: &str) -> String {
    let mut cist = text.trim().to_string();
    if let Some(otvorena) = cist.rfind('(') {
        let unutra = cist[otvorena + 1..].trim_end_matches(')').trim();
        if unutra.len() == 4 && unutra.chars().all(|znak| znak.is_ascii_digit()) {
            cist = cist[..otvorena].trim_end().to_string();
        }
    }
    let rijeci: Vec<&str> = cist.split_whitespace().collect();
    if let Some(zadnja) = rijeci.last()
        && zadnja.len() == 4
        && zadnja.chars().all(|znak| znak.is_ascii_digit())
    {
        cist = rijeci[..rijeci.len() - 1].join(" ");
    }
    cist
}

/// Nađi kandidata bez slike: pretraga + odabir najboljeg pogotka.
pub fn find(agent: &ureq::Agent, query: &Query, api_key: Option<&str>) -> Option<(String, Hit, Source)> {
    let kind = if query.is_series { "tv" } else { "movie" };
    let (hits, source) = match api_key {
        Some(key) => (fetch_api(agent, kind, &query.title, key).unwrap_or_default(), Source::Tmdb),
        None => (fetch_web(agent, kind, &query.title).unwrap_or_default(), Source::TmdbWeb),
    };
    let hit = best_hit(&hits, query)?;
    Some((kind.to_string(), hit, source))
}

#[cfg(test)]
mod detail_tests {
    use super::*;

    #[test]
    fn api_detail_reads_series_fields() {
        let json =
            r#"{"id":1234,"name":"Slow Horses","first_air_date":"2022-04-01","poster_path":"/abc.jpg"}"#;
        let zapis = parse_detail(json).unwrap();
        assert_eq!(zapis.title, "Slow Horses");
        assert_eq!(zapis.year, Some(2022));
        assert_eq!(zapis.poster_hash.as_deref(), Some("abc.jpg"));
    }

    #[test]
    fn api_detail_reads_movie_fields() {
        let json = r#"{"id":273481,"title":"Sicario","release_date":"2015-09-17","poster_path":"/x.jpg"}"#;
        let zapis = parse_detail(json).unwrap();
        assert_eq!((zapis.title.as_str(), zapis.year), ("Sicario", Some(2015)));
    }

    #[test]
    fn web_detail_reads_og_tags() {
        let html = r#"<meta property="og:title" content="Dark Matter (TV Series 2024- ) | TMDB">
            <meta property="og:image" content="https://media.themoviedb.org/t/p/w300_and_h450_bestv2/xyz.jpg">"#;
        let zapis = parse_web_detail(html).unwrap();
        assert_eq!(zapis.title, "Dark Matter");
        assert_eq!(zapis.year, Some(2024));
        assert_eq!(zapis.poster_hash.as_deref(), Some("xyz"));
    }

    #[test]
    fn gallery_picks_most_voted_english_poster() {
        let json = r#"{"posters":[
            {"file_path":"/losa.jpg","iso_639_1":null,"vote_count":0,"vote_average":8.0},
            {"file_path":"/dobra.jpg","iso_639_1":"en","vote_count":12,"vote_average":5.5},
            {"file_path":"/strana.jpg","iso_639_1":"de","vote_count":99,"vote_average":9.0}
        ]}"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let posteri = value.get("posters").unwrap().as_array().unwrap();
        let mut najbolji: Option<(String, u64)> = None;
        for poster in posteri {
            let jezik = poster.get("iso_639_1").and_then(|j| j.as_str()).unwrap_or("");
            if !jezik.is_empty() && jezik != "en" {
                continue;
            }
            let glasova = poster.get("vote_count").and_then(|g| g.as_u64()).unwrap_or(0);
            if najbolji.as_ref().is_none_or(|(_, naj)| glasova > *naj) {
                najbolji = Some((poster.get("file_path").unwrap().as_str().unwrap().to_string(), glasova));
            }
        }
        assert_eq!(najbolji.unwrap().0, "/dobra.jpg", "njemački plakat se preskače");
    }

    #[test]
    fn detail_url_uses_v3_key_in_query() {
        assert_eq!(
            detail_url("tv", "1234", "KEY"),
            "https://api.themoviedb.org/3/tv/1234?api_key=KEY&language=en-US"
        );
    }

    #[test]
    fn exact_title_is_accepted_without_year() {
        assert!(titles_match("Dark Matter", "Dark Matter", Some(2024), Some(2024)));
        assert!(titles_match("The Bureau", "The Bureau", Some(2015), None));
        assert!(titles_match("The Westies", "The Westies", None, None));
    }

    #[test]
    fn far_apart_years_are_a_different_title() {
        // `Fall 2` iz 2026. ne smije se zalijepiti na zapis iz 1970.
        assert!(!titles_match("Fall 2 (2026)", "Fall 2", Some(2026), Some(1970)));
        assert!(!titles_match("Slow Horses", "Slow Horses", Some(2022), Some(2011)));
        assert!(titles_match("Slow Horses", "Slow Horses", Some(2022), Some(2022)));
    }

    #[test]
    fn year_in_title_does_not_break_match() {
        assert!(titles_match("The Fix (2026)", "The Fix", Some(2026), Some(2026)));
        assert!(titles_match("The Fix 2026", "The Fix", Some(2026), Some(2026)));
        assert!(!titles_match("Fall 2 (2026)", "Fall", Some(2026), Some(2022)));
    }

    #[test]
    fn loose_title_needs_matching_year() {
        // Naslov iz datoteke često nosi podnaslov, ali godina mora potvrditi pogodak.
        assert!(titles_match("Sicario", "Sicario: Day of the Soldado", Some(2015), Some(2015)));
        // Nastavak iz druge godine se odbija — to je drugi film.
        assert!(!titles_match("Sicario", "Sicario: Day of the Soldado", Some(2015), Some(2018)));
        // Posve drugačiji naslov se odbija i kad se godina slaže.
        assert!(!titles_match("Slow Horses", "Dark Matter", Some(2024), Some(2024)));
    }
}
