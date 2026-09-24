//! Keš postera na disku: `<config_dir>/art/<id>.<ext>`.
//!
//! Zašto na disku, a ne u bazi: TV-i dohvaćaju poster kao običnu sliku preko
//! `/art/{id}` i mogu je keširati sami; baza pamti samo odakle je došao.

use std::fs;
use std::path::{Path, PathBuf};

/// Mapa s posterima unutar config direktorija.
pub const ART_DIR: &str = "art";

/// Podržane ekstenzije (redoslijed je i redoslijed pretrage).
pub const EXTENSIONS: [&str; 2] = ["jpg", "png"];

/// Putanja postera za objekt.
pub fn poster_path(dir: &Path, item_id: i64, extension: &str) -> PathBuf {
    dir.join(format!("{item_id}.{extension}"))
}

/// Postoji li već poster za taj objekt.
pub fn existing(dir: &Path, item_id: i64) -> Option<PathBuf> {
    EXTENSIONS.iter().map(|extension| poster_path(dir, item_id, extension)).find(|path| path.is_file())
}

/// Upiši poster: prvo u privremenu datoteku, pa `rename` (čitatelj nikad ne vidi pola slike).
pub fn store(dir: &Path, item_id: i64, extension: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let target = poster_path(dir, item_id, extension);
    let temporary = dir.join(format!("{item_id}.{extension}.tmp"));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, &target)?;
    Ok(target)
}

/// Obriši keširani poster (kad objekt nestane iz biblioteke).
pub fn remove(dir: &Path, item_id: i64) -> std::io::Result<()> {
    for extension in EXTENSIONS {
        let path = poster_path(dir, item_id, extension);
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// Veličina keša u bajtovima (za `/api/status`).
pub fn size_on_disk(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .sum()
}

/// Je li dohvaćeno stvarno slika (JPEG ili PNG).
pub fn is_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF]) || bytes.starts_with(&[0x89, b'P', b'N', b'G'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rustiio-art-{}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            name
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];

    #[test]
    fn stores_and_finds_poster() {
        let dir = temp_dir("store");
        assert!(existing(&dir, 7).is_none());

        let path = store(&dir, 7, "jpg", JPEG).expect("upis");
        assert_eq!(path, dir.join("7.jpg"));
        assert_eq!(existing(&dir, 7), Some(path.clone()));

        // Privremena datoteka ne ostaje.
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .expect("mapa")
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());

        assert_eq!(size_on_disk(&dir), JPEG.len() as u64);
        remove(&dir, 7).expect("brisanje");
        assert!(existing(&dir, 7).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn staging_does_not_scan_directory() {
        assert_eq!(size_on_disk(Path::new("/nema/ovoga/na/disku")), 0);
    }

    #[test]
    fn recognizes_images_only() {
        assert!(is_image(JPEG));
        assert!(is_image(&[0x89, b'P', b'N', b'G', 0x0D]));
        assert!(!is_image(b"<!DOCTYPE html><html>404</html>"));
        assert!(!is_image(b""));
    }
}
