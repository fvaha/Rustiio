//! Gdje desktop aplikacija drži svoj config (i uz njega bazu, postere, profile).
//!
//! Isto pravilo kao CLI: config, `rustiio.db` i `art/` žive u istoj mapi.

use std::path::PathBuf;

/// `<config_dir>/config.toml` za ovaj sustav.
pub fn config_path() -> anyhow::Result<PathBuf> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("config.toml"))
}

fn config_dir() -> anyhow::Result<PathBuf> {
    if cfg!(target_os = "macos") {
        return Ok(home()?.join("Library/Application Support/Rustiio"));
    }
    if cfg!(target_os = "windows") {
        let appdata = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("nema APPDATA u okolini"))?;
        return Ok(appdata.join("Rustiio"));
    }
    // Linux i ostalo: prati XDG.
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or(home()?.join(".config"));
    Ok(base.join("rustiio"))
}

fn home() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| anyhow::anyhow!("nema HOME u okolini"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_lands_next_to_the_database() {
        let path = config_path().expect("config putanja");
        assert!(path.ends_with("config.toml"), "config: {}", path.display());
        assert!(path.parent().expect("mapa").is_dir(), "mapa postoji");
    }
}
