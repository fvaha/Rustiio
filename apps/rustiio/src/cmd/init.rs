//! `rustiio init` — napise default config.

use std::path::PathBuf;

use rustiio_core::config::Config;

pub fn execute(config_path: PathBuf) -> anyhow::Result<()> {
    if config_path.exists() {
        println!("\n  Config već postoji: {}", config_path.display());
        println!("  (obriši ga ako želiš svježe defaulte)\n");
        return Ok(());
    }

    let config = Config::default();
    config.save(&config_path)?;

    println!("\n  Config napisan: {}\n", config_path.display());
    println!("  Mape koje sam pogodio (uredi po potrebi):");
    for root in &config.library.roots {
        println!("    - {} -> {}", root.label, root.path.display());
    }
    if config.library.roots.is_empty() {
        println!("    (nijedna uobičajena mapa ne postoji — dodaj svoju pod [[library.roots]])");
    }
    println!("\n  Sljedeće: `rustiio doctor` pa `rustiio run`\n");
    Ok(())
}
