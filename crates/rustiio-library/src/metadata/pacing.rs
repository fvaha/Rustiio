//! Razmak između mrežnih zahtjeva — da nas javni API-ji ne blokiraju.
//!
//! MusicBrainz dokumentira **1 zahtjev u sekundi** i vraća 503 čim se prekorači
//! (provjereno: nakon nekoliko brzih poziva uslijedilo je trajno 503 na neko
//! vrijeme). Zato svaki host ima svoj minimalni razmak, a ne "što brže".
//!
//! Pacing je globalan za proces: server ima jedan radnik za obogaćivanje, pa je
//! jedan red prema van i točan i najjednostavniji.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Razmak prema hostovima koji traže "polako" (MusicBrainz, Cover Art Archive).
pub const SLOW_HOST_GAP: Duration = Duration::from_millis(1200);

/// Razmak prema ostalima (Wikipedia, TVmaze, TMDB).
pub const DEFAULT_GAP: Duration = Duration::from_millis(250);

/// Evidencija zadnjeg zahtjeva po hostu.
#[derive(Debug)]
pub struct Pacing {
    last: Mutex<HashMap<String, Instant>>,
    default_gap: Duration,
}

impl Default for Pacing {
    fn default() -> Self {
        Self::new(DEFAULT_GAP)
    }
}

impl Pacing {
    pub fn new(default_gap: Duration) -> Self {
        Self { last: Mutex::new(HashMap::new()), default_gap }
    }

    /// Pričekaj koliko treba da razmak prema tom hostu bude ispoštovan.
    pub fn wait(&self, url: &str) {
        let host = host_of(url);
        let gap = gap_for(&host, self.default_gap);
        let Ok(mut last) = self.last.lock() else { return };
        if let Some(previous) = last.get(&host) {
            let elapsed = previous.elapsed();
            if elapsed < gap {
                std::thread::sleep(gap - elapsed);
            }
        }
        last.insert(host, Instant::now());
    }
}

/// Pacing koji se dijeli za cijeli proces.
pub fn global() -> &'static Pacing {
    static PACING: OnceLock<Pacing> = OnceLock::new();
    PACING.get_or_init(Pacing::default)
}

/// Host iz URL-a (`https://api.tvmaze.com/x` → `api.tvmaze.com`).
pub fn host_of(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split(['/', '?']).next().unwrap_or(url).to_lowercase()
}

/// Koliki razmak traži taj host.
pub fn gap_for(host: &str, default_gap: Duration) -> Duration {
    const SLOW: [&str; 2] = ["musicbrainz.org", "coverartarchive.org"];
    if SLOW.iter().any(|slow| host.ends_with(slow)) { SLOW_HOST_GAP } else { default_gap }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_host_from_url() {
        assert_eq!(host_of("https://api.tvmaze.com/search/shows?q=x"), "api.tvmaze.com");
        assert_eq!(host_of("https://musicbrainz.org/ws/2/release-group/?fmt=json"), "musicbrainz.org");
        assert_eq!(host_of("nema-sheme"), "nema-sheme");
    }

    #[test]
    fn slow_hosts_get_a_longer_gap() {
        assert_eq!(gap_for("musicbrainz.org", DEFAULT_GAP), SLOW_HOST_GAP);
        assert_eq!(gap_for("coverartarchive.org", DEFAULT_GAP), SLOW_HOST_GAP);
        assert_eq!(gap_for("en.wikipedia.org", DEFAULT_GAP), DEFAULT_GAP);
    }

    #[test]
    fn second_call_to_same_host_waits() {
        let pacing = Pacing::new(Duration::from_millis(120));
        let started = Instant::now();
        pacing.wait("https://primjer.test/prvi");
        pacing.wait("https://primjer.test/drugi");
        assert!(started.elapsed() >= Duration::from_millis(120), "drugi poziv mora pričekati");

        // Drugi host nije vezan čekanjem prvoga.
        let other = Instant::now();
        pacing.wait("https://drugi.test/x");
        assert!(other.elapsed() < Duration::from_millis(60));
    }
}
