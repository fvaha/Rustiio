//! `rustiio posters` — dohvati postere iz konzole (bez web sučelja).
//!
//! Korisno na serveru: pokazuje tocno koji izvor je sto dao, i moze ponovno
//! probati objekte koji su prije ostali bez postera (`--reset`).

use std::path::{Path, PathBuf};

use anyhow::Context;
use rustiio_core::config::Config;
use rustiio_library::Store;
use rustiio_library::metadata::Enricher;

use crate::cli::PostersArgs;

pub async fn execute(config_path: PathBuf, args: PostersArgs) -> anyhow::Result<()> {
    let mut config = Config::load_or_create(&config_path)?;
    let (ffmpeg, ffprobe) = config.resolve_tools();
    tracing::debug!(%ffmpeg, %ffprobe, "alati");

    let store = open_store(&config_path)?;
    if let Ok(cleared) = rustiio_library::store::items::reset_missing_posters(&store) {
        if args.reset && cleared > 0 {
            println!("zaboravljeno {cleared} objekata oznacenih kao \"nema ga\"");
        }
    }

    let art_dir = config_path.parent().unwrap_or_else(|| Path::new(".")).join("art");
    let enricher = Enricher::from_env(art_dir.clone(), &config.network.ip_family);

    println!(
        "posteri: ked={} kljuc={}",
        art_dir.display(),
        if enricher.has_api_key() { "da" } else { "ne (keyless izvori)" }
    );

    let summary = rustiio_library::metadata::run_until_done(
        &store,
        &enricher,
        args.batch.max(1),
        args.mark_missing,
        args.max_batches.max(1),
    )?;

    println!("obradeno={} dohvaceno={} bez_postera={}", summary.processed, summary.found, summary.missing);
    if summary.processed == 0 {
        println!(
            "nema videa bez postera — sve ima sliku (ili su oznaceni kao \"nema ga\"; --reset probaj ponovno)"
        );
    }
    Ok(())
}

/// Isto kao u `run`: baza ide uz config (`RUSTIIO_DB` pregazi).
fn open_store(config_path: &Path) -> anyhow::Result<Store> {
    let path = std::env::var_os("RUSTIIO_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| config_path.parent().unwrap_or_else(|| Path::new(".")).join("rustiio.db"));
    Store::open(&path).with_context(|| format!("SQLite baza {}", path.display()))
}
