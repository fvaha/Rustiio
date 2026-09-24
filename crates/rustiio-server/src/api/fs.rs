//! Preglednik mapa na serveru — korisnik bira mapu s videom, bez tipkanja putanje.
//!
//! Isti postupak tako radi i u browseru i u desktop aplikaciji (sučelje je isto),
//! a gleda datotečni sustav onog stroja na kojem server radi — što je i jedino
//! mjesto gdje biblioteka postoji.

use std::path::{Path, PathBuf};

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Najviše podmapa u jednom odgovoru (velike mape ne smiju zamrznuti sučelje).
const LIMIT: usize = 500;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/fs/list", get(list))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// Mapa koju pregledavamo; prazno znači početna mapa korisnika.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Shortcut {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    /// Koliko videa ima izravno u toj mapi (korisnik tako vidi gdje mu je gradivo).
    pub videos: usize,
    /// Koliko podmapa ima.
    pub folders: usize,
}

#[derive(Debug, Serialize)]
pub struct Listing {
    pub path: String,
    pub parent: Option<String>,
    pub shortcuts: Vec<Shortcut>,
    /// Videa izravno u ovoj mapi.
    pub videos: usize,
    pub dirs: Vec<Entry>,
    pub error: Option<String>,
}

async fn list(State(state): State<AppState>, Query(query): Query<ListQuery>) -> Json<Listing> {
    let wanted = query.path.as_deref().map(str::trim).filter(|path| !path.is_empty());
    let current = wanted.map(expand).unwrap_or_else(start_dir);
    let extensions = state.config.library.video_extensions.clone();
    Json(read_dir(&current, &extensions))
}

/// `~` i prazno → početna mapa korisnika.
fn expand(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~") {
        if let Some(home) = home() {
            return home.join(rest.trim_start_matches(['/', '\\']));
        }
    }
    PathBuf::from(path)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

fn start_dir() -> PathBuf {
    home().unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }))
}

/// Pročitaj jednu mapu: podmape, broj videa i prečace.
fn read_dir(dir: &Path, extensions: &[String]) -> Listing {
    let shortcuts = shortcuts();
    let mut listing = Listing {
        path: dir.display().to_string(),
        parent: dir.parent().map(|parent| parent.display().to_string()),
        shortcuts,
        videos: 0,
        dirs: Vec::new(),
        error: None,
    };

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            listing.error = Some(format!("{}: {error}", dir.display()));
            return listing;
        }
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue; // skriveno ne zanima nikoga tko traži filmove
        }
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if !is_dir {
            if is_video(&name, extensions) {
                listing.videos += 1;
            }
            continue;
        }
        if listing.dirs.len() >= LIMIT {
            continue;
        }
        let path = entry.path();
        let (videos, folders) = count_inside(&path, extensions);
        listing.dirs.push(Entry { name, path: path.display().to_string(), videos, folders });
    }

    listing.dirs.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    listing
}

/// Broj videa i podmapa izravno u mapi (bez zalaženja dublje).
fn count_inside(dir: &Path, extensions: &[String]) -> (usize, usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return (0, 0) };
    let mut videos = 0;
    let mut folders = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            folders += 1;
        } else if is_video(&name, extensions) {
            videos += 1;
        }
    }
    (videos, folders)
}

fn is_video(name: &str, extensions: &[String]) -> bool {
    let lower = name.to_lowercase();
    extensions.iter().any(|extension| lower.ends_with(&format!(".{}", extension.to_lowercase())))
}

/// Mjesta na koja korisnik najčešće stavlja video (samo ona koja postoje).
fn shortcuts() -> Vec<Shortcut> {
    let mut wanted: Vec<(&str, PathBuf)> = Vec::new();
    if let Some(home) = home() {
        wanted.push(("Početna", home.clone()));
        for name in ["Movies", "Videos", "Downloads", "Filmovi", "Serije"] {
            wanted.push((name, home.join(name)));
        }
    }
    if cfg!(target_os = "macos") {
        wanted.push(("Vanjska jedinica", PathBuf::from("/Volumes")));
    }
    wanted.push(("Mediji", PathBuf::from("/media")));
    wanted.push(("Montirano", PathBuf::from("/mnt")));
    if cfg!(windows) {
        wanted.push(("C:", PathBuf::from("C:\\")));
    } else {
        wanted.push(("Korijen", PathBuf::from("/")));
    }

    let mut out: Vec<Shortcut> = Vec::new();
    for (name, path) in wanted {
        if !path.is_dir() || out.iter().any(|item| item.path == path.display().to_string()) {
            continue;
        }
        out.push(Shortcut { name: name.to_string(), path: path.display().to_string() });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("rustiio-fs-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("Serije/Zlo")).expect("mape");
        std::fs::write(path.join("Film.mkv"), b"x").expect("film");
        std::fs::write(path.join("Serije/Zlo/S01E01.mkv"), b"x").expect("epizoda");
        std::fs::write(path.join("biljeska.txt"), b"x").expect("tekst");
        path
    }

    #[test]
    fn lists_only_folders_and_counts_videos() {
        let dir = temp_dir("popis");
        let listing = read_dir(&dir, &["mkv".to_string(), "mp4".to_string()]);
        assert!(listing.error.is_none(), "{:?}", listing.error);
        assert_eq!(listing.videos, 1, "samo Film.mkv je video");
        let names: Vec<String> = listing.dirs.iter().map(|entry| entry.name.clone()).collect();
        assert_eq!(names, vec!["Serije"], "txt nije mapa i ne prikazuje se");
        assert_eq!(listing.dirs[0].videos, 0, "epizoda je dublje, u Zlo");
        assert_eq!(listing.dirs[0].folders, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hidden_folders_are_skipped() {
        let dir = temp_dir("skriveno");
        std::fs::create_dir_all(dir.join(".cache")).expect("mapa");
        let listing = read_dir(&dir, &["mkv".to_string()]);
        assert!(!listing.dirs.iter().any(|entry| entry.name == ".cache"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_folder_reports_error_and_keeps_shortcuts() {
        let listing = read_dir(Path::new("/nema-ovoga-stvarno"), &[]);
        let message = listing.error.expect("greska");
        assert!(message.contains("/nema-ovoga-stvarno"), "{message}");
        assert!(listing.dirs.is_empty());
    }

    #[test]
    fn expansion_and_start_dir_work() {
        let expanded = expand("~");
        assert_eq!(expanded, home().expect("home"));
        assert!(start_dir().is_absolute());
    }

    #[test]
    fn video_matching_ignores_case() {
        assert!(is_video("Film.MKV", &["mkv".to_string()]));
        assert!(!is_video("Film.mkv", &["mp4".to_string()]));
    }
}
