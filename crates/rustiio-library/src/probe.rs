//! Lijeno ocitavanje trajanja preko `ffprobe`.
//!
//! Potrebno je samo za `TimeSeekRange` (seek po vremenu) — bez trajanja ne mozemo
//! pretvoriti "14:20" u bajt. Radi se lijeno (tek kad TV zatrzi seek) i s cacheom,
//! da start servera ne ovisi o ffprobe-u nad cijelom bibliotekom. Faza 3 ovo seli
//! u SQLite indeks i puni ga tijekom skeniranja.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use tracing::{debug, warn};

pub struct DurationProbe {
    program: String,
    enabled: bool,
    cache: Mutex<HashMap<PathBuf, Option<u64>>>,
    max_entries: usize,
}

impl DurationProbe {
    pub fn new(program: impl Into<String>, enabled: bool) -> Self {
        Self { program: program.into(), enabled, cache: Mutex::new(HashMap::new()), max_entries: 10_000 }
    }

    /// Trajanje u milisekundama, ili `None` ako ffprobe nije dostupan/ne zna procitati.
    pub fn duration_ms(&self, path: &Path) -> Option<u64> {
        if !self.enabled {
            return None;
        }
        if let Ok(cache) = self.cache.lock() {
            if let Some(hit) = cache.get(path) {
                return *hit;
            }
        }

        let result = run_ffprobe(&self.program, path);
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= self.max_entries {
                cache.clear(); // Faza 3: SQLite s LRU-om
            }
            cache.insert(path.to_path_buf(), result);
        }
        result
    }

    pub fn cached_entries(&self) -> usize {
        self.cache.lock().map(|cache| cache.len()).unwrap_or(0)
    }

    /// Ocisti cache (npr. nakon reskeniranja).
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }
}

fn run_ffprobe(program: &str, path: &Path) -> Option<u64> {
    let output = Command::new(program)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            parse_ffprobe_duration(&String::from_utf8_lossy(&output.stdout))
        }
        Ok(output) => {
            debug!(
                path = %path.display(),
                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                "ffprobe nije vratio trajanje"
            );
            None
        }
        Err(err) => {
            warn!(program = program, error = %err, "ffprobe se ne moze pokrenuti (trajanje nedostupno)");
            None
        }
    }
}

/// `20.023000` -> `20023` ms. Vraca `None` za prazno, `N/A` i negativno.
pub fn parse_ffprobe_duration(stdout: &str) -> Option<u64> {
    let raw = stdout.lines().map(str::trim).find(|line| !line.is_empty())?;
    let seconds: f64 = raw.parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some((seconds * 1000.0).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_seconds_with_fraction() {
        assert_eq!(parse_ffprobe_duration("20.023000\n"), Some(20_023));
        assert_eq!(parse_ffprobe_duration("7380.5"), Some(7_380_500));
        assert_eq!(parse_ffprobe_duration("0.000000"), Some(0));
    }

    #[test]
    fn rejects_junk() {
        assert_eq!(parse_ffprobe_duration(""), None);
        assert_eq!(parse_ffprobe_duration("N/A\n"), None);
        assert_eq!(parse_ffprobe_duration("-1"), None);
        assert_eq!(parse_ffprobe_duration("inf"), None);
    }

    #[test]
    fn disabled_probe_never_touches_the_process_or_cache() {
        let probe = DurationProbe::new("ffprobe-definitivno-ne-postoji", false);
        assert_eq!(probe.duration_ms(Path::new("/tmp/nema.mkv")), None);
        assert_eq!(probe.cached_entries(), 0, "iskljucen probe ne smije puniti cache");
    }

    #[test]
    fn missing_binary_is_cached_as_none_without_panic() {
        let probe = DurationProbe::new("ffprobe-definitivno-ne-postoji", true);
        assert_eq!(probe.duration_ms(Path::new("/tmp/x.mkv")), None);
        assert_eq!(probe.duration_ms(Path::new("/tmp/x.mkv")), None, "drugi poziv ide iz cachea");
        assert_eq!(probe.cached_entries(), 1);
        probe.clear();
        assert_eq!(probe.cached_entries(), 0);
    }

    /// Stvarni ffprobe nad stvarnim fajlom — preskace se ako ffprobe nije u PATH-u.
    #[test]
    fn real_ffprobe_measures_generated_file() {
        let dir = std::env::temp_dir().join(format!("rustiio-probe-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("test.mkv");
        let generated = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x120:rate=10:duration=2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&file)
            .status();
        if !matches!(generated, Ok(status) if status.success()) {
            eprintln!("ffmpeg nije dostupan — preskacem");
            return;
        }

        let probe = DurationProbe::new("ffprobe", true);
        let duration = probe.duration_ms(&file).expect("trajanje procitano");
        assert!((1_500..=2_500).contains(&duration), "ocekivano ~2000 ms, dobili {duration}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
