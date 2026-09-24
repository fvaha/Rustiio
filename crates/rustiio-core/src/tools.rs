//! Gdje su vanjski alati (ffmpeg, ffprobe).
//!
//! Pravilo: **prvo idu oni uz nas binarni fajl** (`<dir>/ffmpeg`), pa tek onda
//! onaj iz configa / `PATH`-a. Tako paket radi odmah po instaliranju i ne
//! ovisi o tome što je korisnik prije instalirao.
//!
//! Eksplicitna putanja u configu (npr. `/usr/local/bin/ffmpeg`) uvijek pobjeđuje
//! — ako je netko nešto upisao, to je njegova odluka.

use std::path::{Path, PathBuf};

/// Mape u kojima tražimo priložene alate, uz mapu binarnog fajla.
const BUNDLE_DIRS: [&str; 3] = ["", "../lib/rustiio", "../share/rustiio"];

/// Mapa u kojoj je naš binarni fajl.
pub fn exe_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.to_path_buf())
}

/// Je li vrijednost iz configa "samo ime" (`ffmpeg`) ili putanja?
///
/// `ffmpeg.exe` na Windowsima je i dalje samo ime.
pub fn is_bare_name(configured: &str) -> bool {
    std::path::Path::new(configured).file_name().map(|name| name.to_string_lossy().to_string())
        == Some(configured.to_string())
}

/// Traži priloženi alat u mapama uz binarni fajl (`base=<mapa binarnog fajla>`).
pub fn find_bundled_in(base: &Path, name: &str) -> Option<PathBuf> {
    let mut candidates = vec![name.to_string()];
    // Na Windowsima je prilozeni alat `ffmpeg.exe`.
    if !std::env::consts::EXE_SUFFIX.is_empty() {
        candidates.push(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    }

    BUNDLE_DIRS.iter().find_map(|relative| {
        let dir = if relative.is_empty() { base.to_path_buf() } else { base.join(relative) };
        candidates.iter().map(|file| dir.join(file)).find(|path| path.is_file())
    })
}

/// Konačna putanja alata: priloženi uz binarni fajl, inače ono iz configa.
pub fn resolve_in(base: Option<&Path>, configured: &str) -> String {
    if !is_bare_name(configured) {
        return configured.to_string();
    }
    base.and_then(|base| find_bundled_in(base, configured))
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| configured.to_string())
}

/// Isto kao [`resolve_in`], ali s mapom stvarnog binarnog fajla.
pub fn resolve(configured: &str) -> String {
    resolve_in(exe_dir().as_deref(), configured)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustiio-tools-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mapa");
        dir
    }

    #[test]
    fn bare_name_is_recognized() {
        assert!(is_bare_name("ffmpeg"));
        assert!(is_bare_name("ffprobe"));
        assert!(!is_bare_name("/usr/local/bin/ffmpeg"));
        assert!(!is_bare_name("./ffmpeg"));
        assert!(!is_bare_name("bin/ffmpeg"));
    }

    #[test]
    fn explicit_path_always_wins() {
        let dir = temp_dir("explicit");
        std::fs::write(dir.join("ffmpeg"), b"x").expect("datoteka");
        assert_eq!(resolve_in(Some(&dir), "/opt/ffmpeg"), "/opt/ffmpeg");
    }

    #[test]
    fn bundled_tool_next_to_the_binary_wins_over_path() {
        let dir = temp_dir("bundled");
        std::fs::write(dir.join("ffmpeg"), b"x").expect("datoteka");
        assert_eq!(resolve_in(Some(&dir), "ffmpeg"), dir.join("ffmpeg").to_string_lossy());

        // Bez prilozenog alata ostaje ime (dakle PATH).
        assert_eq!(resolve_in(Some(&dir), "ffprobe"), "ffprobe");
    }

    #[test]
    fn bundled_tool_is_found_in_lib_subdir() {
        let dir = temp_dir("lib");
        let lib = dir.join("../lib/rustiio");
        std::fs::create_dir_all(&lib).expect("mapa");
        std::fs::write(lib.join("ffprobe"), b"x").expect("datoteka");
        assert_eq!(resolve_in(Some(&dir), "ffprobe"), dir.join("../lib/rustiio/ffprobe").to_string_lossy());
    }
}
