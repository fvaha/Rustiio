//! `rustiio run` — digne HTTP server i SSDP oglasavanje.
//!
//! Sav posao oko dizanja (config, katalog, baza, profili, SSDP, pozadinski poslovi)
//! je u `rustiio_server::boot` — isto koristi i desktop aplikacija.

use std::path::PathBuf;

use rustiio_server::{BootOptions, boot};
use tracing::info;

use crate::cli::RunArgs;

pub async fn execute(config_path: PathBuf, args: RunArgs) -> anyhow::Result<()> {
    let options = BootOptions { scan: !args.no_scan, ssdp: !args.no_ssdp, ..BootOptions::default() };
    let booted = boot(config_path.clone(), options).await?;
    let base_url = booted.base_url.clone();
    let friendly_name = booted.identity.friendly_name.clone();
    info!(addr = %booted.addr(), "server slusa");

    // Dashboard u web sučelju čita CPU/RAM/diskove iz ovog uzorkivača.
    rustiio_server::api::stats::start_sampler();

    print_banner(&friendly_name, &base_url, &config_path);

    booted.serve(shutdown_signal()).await
}

fn print_banner(friendly_name: &str, base_url: &str, config_path: &std::path::Path) {
    println!();
    println!("  {} v{}", rustiio_core::APP_NAME, rustiio_core::VERSION);
    println!("  uređaj:      {friendly_name}");
    println!("  web:         {base_url}/");
    println!("  status:      {base_url}/healthz");
    println!("  DLNA:        {base_url}/rootDesc.xml");
    println!("  config:      {}", config_path.display());
    println!("  (na TV-u otvori DLNA/UPnP izvor \"{friendly_name}\"; VLC: Local Network)");
    println!();
}

/// Ctrl+C ili SIGTERM (systemd) — uredno ugasi SSDP i posalje byebye.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("gasim se");
}
