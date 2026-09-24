//! `rustiio service` — pusti server kao uslugu (systemd / launchd / Windows servis).
//!
//! Bez ovoga server radi samo dok je terminal otvoren; s ovim se diže sam nakon
//! prijave (macOS/Linux korisnička usluga) ili podizanja stroja (root/Windows).
//! Kod je po platformama u zasebnim modulima — zajedničko je samo ožičenje.

// Moduli su namjerno svi prevedeni na svakoj platformi: generatore sadržaja
// (unit/plist/sc create) tako testiraju testovi na svim sustavima, a ne samo
// na onom gdje se izvršavaju.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub mod launchd;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub mod systemd;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub mod windows;

use std::path::PathBuf;

use anyhow::Context;

/// Što radi `rustiio service`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Action {
    /// Upiši uslugu i pokreni je.
    Install,
    /// Zaustavi uslugu i obriši njezine datoteke.
    Uninstall,
    /// Reci je li usluga upisana i radi li.
    Status,
}

/// Pokreni zadanu radnju za ovaj sustav.
pub fn execute(action: Action, config_path: PathBuf) -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("putanja do vlastitog programa")?;
    let exe = exe.canonicalize().unwrap_or(exe);

    #[cfg(target_os = "macos")]
    {
        launchd::run(action, &exe, &config_path)
    }
    #[cfg(target_os = "linux")]
    {
        systemd::run(action, &exe, &config_path)
    }
    #[cfg(target_os = "windows")]
    {
        windows::run(action, &exe, &config_path)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (action, exe, config_path);
        anyhow::bail!("ova platforma nema podrsku za uslugu")
    }
}

/// Argumenti kojima usluga diže server (isto za sve platforme).
pub fn server_args(exe: &std::path::Path, config_path: &std::path::Path) -> Vec<String> {
    vec![
        exe.display().to_string(),
        "run".to_string(),
        "--config".to_string(),
        config_path.display().to_string(),
    ]
}

/// Ispiši sažetak stanja (koristi ga svaki platformski modul).
pub fn report(platform: &str, file: &std::path::Path, active: bool, hint: &str) {
    println!();
    println!("  Rustiio usluga ({platform})");
    println!("  datoteka:    {}", file.display());
    println!("  upisana:     {}", if file.exists() { "da" } else { "ne" });
    println!("  radi:        {}", if active { "da" } else { "ne" });
    println!("  {hint}");
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_args_point_at_this_exe_and_config() {
        let args =
            server_args(std::path::Path::new("/opt/Rustiio/rustiio"), std::path::Path::new("/tmp/c.toml"));
        assert_eq!(args, vec!["/opt/Rustiio/rustiio", "run", "--config", "/tmp/c.toml"]);
    }
}
