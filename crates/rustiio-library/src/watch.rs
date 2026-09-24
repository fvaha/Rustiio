//! Praćenje mapa: kad se datoteka doda ili obriše, biblioteka se sama osvježi.
//!
//! Zašto `notify`: inotify (Linux), FSEvents (macOS) i `ReadDirectoryChangesW`
//! (Windows) daju događaj odmah, pa nema periodičnog skeniranja cijelog diska.
//! Sken ipak ostaje izvor istine — watcher samo kaže "sad provjeri".

use std::path::PathBuf;
use std::time::Duration;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{info, warn};

/// Koliko tišine znači da je kopiranje gotovo (veliki fajlovi stižu u komadima).
pub const DEFAULT_QUIET: Duration = Duration::from_secs(5);

/// Watcher nad mapama korijena.
pub struct LibraryWatcher {
    _watcher: RecommendedWatcher,
    roots: Vec<PathBuf>,
    quiet: Duration,
}

impl LibraryWatcher {
    /// Počni pratiti `roots` i zovi `on_change` kad se sadržaj promijeni.
    ///
    /// Vraća `Ok(None)`-ekvivalentnu situaciju kao grešku koju pozivatelj smije
    /// ignorirati: bez watchera server i dalje radi (resken na zahtjev).
    pub fn start<F>(roots: &[PathBuf], quiet: Duration, mut on_change: F) -> notify::Result<Self>
    where
        F: FnMut() + Send + 'static,
    {
        let quiet_for_callback = quiet;
        let mut last: Option<std::time::Instant> = None;
        let mut watcher = RecommendedWatcher::new(
            move |result: notify::Result<Event>| match result {
                Ok(event) => {
                    if !touches_library(&event) {
                        return;
                    }
                    // Debounce: ne javljaj dok se promjene ne stišaju.
                    let now = std::time::Instant::now();
                    let settled =
                        last.is_none_or(|previous| now.duration_since(previous) >= quiet_for_callback);
                    last = Some(now);
                    if settled {
                        on_change();
                    }
                }
                Err(error) => warn!(error = %error, "greska pri pracenju mapa"),
            },
            Config::default(),
        )?;

        for root in roots {
            if !root.exists() {
                continue;
            }
            if let Err(error) = watcher.watch(root, RecursiveMode::Recursive) {
                warn!(root = %root.display(), error = %error, "mapa se ne moze pratiti");
            } else {
                info!(root = %root.display(), "pratim mapu");
            }
        }

        Ok(Self { _watcher: watcher, roots: roots.to_vec(), quiet })
    }

    /// Mape koje pratimo (za ispis u logu).
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Koliko tišine čekamo prije nego javimo promjenu.
    pub fn quiet(&self) -> Duration {
        self.quiet
    }
}

/// Zanima nas samo dodavanje/brisanje/premještanje datoteka, ne pristup i metapodaci.
fn touches_library(event: &Event) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(notify::event::ModifyKind::Name(_))
    )
}

/// Jesu li ovo mape vrijedne praćenja (postoje i nisu prazne).
pub fn watchable_roots(candidates: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = candidates.iter().filter(|path| path.is_dir()).cloned().collect();
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchable_roots_keeps_only_directories() {
        let dir = std::env::temp_dir().join(format!("rustiio-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mapa");
        let file = dir.join("film.mkv");
        std::fs::write(&file, b"x").expect("fajl");

        let roots = watchable_roots(&[dir.clone(), file.clone(), dir.join("nema")]);
        assert_eq!(roots, vec![dir.clone()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_event_triggers_change_report() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let dir = std::env::temp_dir().join(format!("rustiio-watch-fire-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mapa");

        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let watcher =
            LibraryWatcher::start(std::slice::from_ref(&dir), Duration::from_millis(50), move || {
                counter.fetch_add(1, Ordering::SeqCst);
            })
            .expect("watcher");
        assert_eq!(watcher.roots().len(), 1);

        std::fs::write(dir.join("novi.mkv"), b"x").expect("fajl");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while hits.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(hits.load(Ordering::SeqCst) >= 1, "novi fajl mora javiti promjenu");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
