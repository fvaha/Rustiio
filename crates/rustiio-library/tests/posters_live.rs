//! Živi testovi dohvata postera — traže mrežu, zato su `#[ignore]`.
//!
//! Pokretanje: `cargo test -p rustiio-library --test posters_live -- --ignored --nocapture`
//!
//! Ovo je dokaz da lanac radi bez ijednog ključa (TMDB web, Wikipedia, TVmaze,
//! Cover Art Archive) i da lokalni poster ima prednost nad mrežom.

use std::path::PathBuf;

use rustiio_library::metadata::{cache, keyless, local};
use rustiio_library::{Enricher, Query, guess_title};

fn temp_art(name: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "rustiio-live-art-{}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        name
    ));
    let _ = std::fs::remove_dir_all(&path);
    path
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(25)))
        .user_agent(keyless::USER_AGENT)
        .build();
    config.into()
}

fn report(label: &str, query: &Query, source: Option<&str>, bytes: usize) {
    println!(
        "  {label}: naslov={:?} godina={:?} serija={} → izvor={:?} bajtova={bytes}",
        query.title, query.year, query.is_series, source
    );
}

#[test]
#[ignore = "trazi mrezu"]
fn tmdb_web_is_enough_for_a_movie() {
    let art = temp_art("tmdb-web");
    let enricher = Enricher::new(None, art.clone());
    assert!(!enricher.has_api_key(), "ovo je test bez kljuca");

    let query = guess_title("Sicario.2015.1080p.BluRay.x264-[YTS.AM].mkv");
    assert_eq!(query.title, "Sicario");
    let poster = enricher.poster_for_query(1, &query).expect("poster s TMDB-a (bez kljuca)");
    let bytes = std::fs::read(&poster.path).expect("poster na disku");
    report("TMDB web", &query, poster.source.map(|source| source.as_str()), bytes.len());
    assert!(bytes.len() > 10_000, "premala slika: {} bajtova", bytes.len());
    assert!(cache::is_image(&bytes));
    assert_eq!(poster.source.map(|source| source.as_str()), Some("tmdb-web"));
}

#[test]
#[ignore = "trazi mrezu"]
fn cached_poster_is_reused_without_download() {
    let art = temp_art("cache");
    let enricher = Enricher::new(None, art.clone());
    let query = Query { title: "The Matrix".to_string(), year: Some(1999), is_series: false };
    let first = enricher.poster_for_query(2, &query).expect("prvi dohvat");
    let second = enricher.poster_for_query(2, &query).expect("drugi dohvat");
    assert_eq!(first.path, second.path);
    assert!(second.source.is_none(), "drugi put ne ide na mrezu");
    assert_eq!(cache::existing(&art, 2), Some(second.path));
}

#[test]
#[ignore = "trazi mrezu"]
fn wikipedia_covers_films_tmdb_may_miss() {
    let agent = agent();
    // Bez dijakritike: članak je `Amélie`, upit je `Amelie` — točan naslov ne prolazi,
    // pa posao mora odraditi pretraga.
    let query = Query { title: "Amelie".to_string(), year: Some(2001), is_series: false };
    let poster = keyless::wikipedia_poster(&agent, &query);
    let poster = poster.expect("poster s Wikipedije");
    report("Wikipedia", &query, Some("wikipedia"), poster.bytes.len());
    assert!(cache::is_image(&poster.bytes));
    assert!(poster.url.contains("wikimedia") || poster.url.contains("wikipedia"));
}

#[test]
#[ignore = "trazi mrezu"]
fn tvmaze_and_coverart_work_without_keys() {
    let agent = agent();

    let series = Query { title: "Dark Matter".to_string(), year: Some(2024), is_series: true };
    let poster = keyless::tvmaze_poster(&agent, &series).expect("poster sa TVmaze");
    report("TVmaze", &series, Some("tvmaze"), poster.bytes.len());
    assert!(poster.bytes.len() > 10_000);

    let album = Query { title: "Nevermind".to_string(), year: None, is_series: false };
    let cover = keyless::coverart_poster(&agent, &album).expect("naslovnica s Cover Art Archive");
    report("Cover Art", &album, Some("coverart"), cover.bytes.len());
    assert!(cache::is_image(&cover.bytes));
}

#[test]
#[ignore = "trazi mrezu"]
fn local_poster_beats_the_network() {
    let art = temp_art("local");
    let media = temp_art("local-media");
    std::fs::create_dir_all(&media).expect("mapa");

    let video = media.join("Sicario.2015.mkv");
    std::fs::write(&video, b"film").expect("video");
    let mut poster = vec![0xFF, 0xD8, 0xFF, 0xE0];
    poster.resize(4096, 0x7F);
    std::fs::write(media.join("poster.jpg"), &poster).expect("poster uz film");

    let enricher = Enricher::new(None, art.clone());
    let found = enricher.poster_for(3, &video).expect("lokalni poster");
    assert_eq!(found.source.map(|source| source.as_str()), Some("local"));
    assert_eq!(found.bytes, poster.len());
    let stored = std::fs::read(&found.path).expect("u kesu");
    assert_eq!(stored, poster, "lokalna slika je kopirana u kes bez promjene");
    assert!(local::find_local(&video).is_some());

    let _ = std::fs::remove_dir_all(&media);
}

#[test]
#[ignore = "trazi mrezu i TMDB_API_KEY"]
fn api_key_uses_the_official_tmdb_api() {
    let key = std::env::var("TMDB_API_KEY").expect("TMDB_API_KEY u okolini");
    let art = temp_art("tmdb-api");
    let enricher = Enricher::new(Some(key), art);
    assert!(enricher.has_api_key());

    let query = guess_title("Sicario.2015.1080p.BluRay.x264-[YTS.AM].mkv");
    let poster = enricher.poster_for_query(11, &query).expect("poster preko API-ja");
    let bytes = std::fs::read(&poster.path).expect("poster na disku");
    report("TMDB API", &query, poster.source.map(|source| source.as_str()), bytes.len());
    assert!(cache::is_image(&bytes));
    // S kljucem ide API, ne web scrape.
    assert_eq!(poster.source.map(|source| source.as_str()), Some("tmdb"));
}
