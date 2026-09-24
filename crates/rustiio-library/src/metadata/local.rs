//! Poster koji već leži uz datoteku — bez ijednog mrežnog poziva.
//!
//! Konvencija koju koriste Kodi, Emby i Serviio (i koju ljudi stvarno imaju na
//! disku): `poster.jpg`, `folder.jpg`, `cover.jpg`, `movie.jpg`, ili slika s
//! istim imenom kao video (`Sicario.2015.jpg`). Za serije vrijedi i naziv mape
//! (`Dark Matter/poster.jpg`).

use std::fs;
use std::path::{Path, PathBuf};

/// Imena koja se traže u mapi datoteke.
pub const POSTER_STEMS: [&str; 5] = ["poster", "folder", "cover", "movie", "default"];

/// Ekstenzije slika uz video.
pub const IMAGE_EXTENSIONS: [&str; 3] = ["jpg", "jpeg", "png"];

/// Nađi lokalni poster za video datoteku i vrati (putanja, bajtovi).
pub fn find_local(video: &Path) -> Option<(PathBuf, Vec<u8>)> {
    let dir = video.parent()?;

    // 1. Isto ime kao video: `Sicario.2015.mkv` → `Sicario.2015.jpg`.
    if let Some(stem) = video.file_stem().and_then(|stem| stem.to_str()) {
        for extension in IMAGE_EXTENSIONS {
            let candidate = dir.join(format!("{stem}.{extension}"));
            if let Some(bytes) = read_image(&candidate) {
                return Some((candidate, bytes));
            }
        }
    }

    // 2. Uobičajena imena postera u istoj mapi.
    for stem in POSTER_STEMS {
        for extension in IMAGE_EXTENSIONS {
            let candidate = dir.join(format!("{stem}.{extension}"));
            if let Some(bytes) = read_image(&candidate) {
                return Some((candidate, bytes));
            }
        }
    }

    // 3. Naziv mape (za serije: `Serije/Dark Matter/poster.jpg`).
    if let Some(folder) = dir.file_name().and_then(|name| name.to_str()) {
        for extension in IMAGE_EXTENSIONS {
            let candidate = dir.join(format!("{folder}.{extension}"));
            if let Some(bytes) = read_image(&candidate) {
                return Some((candidate, bytes));
            }
        }
    }

    None
}

/// Pročitaj datoteku samo ako je stvarno slika i nije prazna.
fn read_image(path: &Path) -> Option<Vec<u8>> {
    let bytes = fs::read(path).ok()?;
    (super::cache::is_image(&bytes) && bytes.len() > 1024).then_some(bytes)
}

/// Ekstenzija po sadržaju (za imenovanje u kešu).
pub fn extension_of(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "png" } else { "jpg" }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0];
    fn image(size: usize) -> Vec<u8> {
        let mut bytes = JPEG.to_vec();
        bytes.resize(size, 0);
        bytes
    }

    fn temp_dir(name: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rustiio-local-art-{}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            name
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("mapa");
        path
    }

    #[test]
    fn finds_poster_named_after_the_video() {
        let dir = temp_dir("same-name");
        let video = dir.join("Sicario.2015.mkv");
        fs::write(&video, b"film").expect("video");
        fs::write(dir.join("Sicario.2015.jpg"), image(2000)).expect("slika");

        let (path, bytes) = find_local(&video).expect("poster");
        assert_eq!(path, dir.join("Sicario.2015.jpg"));
        assert_eq!(bytes.len(), 2000);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_to_folder_poster() {
        let dir = temp_dir("folder-poster");
        let video = dir.join("Epizoda 1.mp4");
        fs::write(&video, b"epizoda").expect("video");
        fs::write(dir.join("folder.jpg"), image(1500)).expect("slika");

        assert!(find_local(&video).is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ignores_tiny_or_non_image_files() {
        let dir = temp_dir("junk");
        let video = dir.join("Film.mkv");
        fs::write(&video, b"film").expect("video");
        // HTML preimenovan u .jpg (čest slučaj kod krivih scrapera).
        fs::write(dir.join("poster.jpg"), b"<!DOCTYPE html>").expect("lazni poster");
        assert!(find_local(&video).is_none());

        fs::write(dir.join("poster.jpg"), image(10)).expect("premala slika");
        assert!(find_local(&video).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_poster_means_none() {
        let dir = temp_dir("nema");
        let video = dir.join("Film.mkv");
        fs::write(&video, b"film").expect("video");
        assert!(find_local(&video).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
