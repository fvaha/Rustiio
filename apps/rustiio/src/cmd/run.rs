//! `rustiio run` — digne HTTP server i SSDP oglasavanje.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use rustiio_core::config::Config;
use rustiio_core::net;
use rustiio_core::{
    DEVICE_TYPE, DeviceIdentity, SERVICE_CONNECTION_MANAGER, SERVICE_CONTENT_DIRECTORY, server_header,
};
use rustiio_library::{ScanOptions, scan};
use rustiio_profiles::builtin;
use rustiio_server::{AppState, router};
use rustiio_ssdp::{SsdpConfig, start as start_ssdp};
use tokio::net::TcpListener;
use tracing::{info, warn};

use crate::cli::RunArgs;

pub async fn execute(config_path: PathBuf, args: RunArgs) -> anyhow::Result<()> {
    let mut config = Config::load_or_create(&config_path).context("ucitavanje configa")?;
    let identity = DeviceIdentity::ensure(&mut config, &config_path)?;
    let ip = resolve_ip(&config)?;
    let port = config.server.http_port;
    let base_url = format!("http://{ip}:{port}");

    let roots = config.existing_roots();
    if roots.is_empty() {
        warn!("nijedna medijska mapa iz [library].roots ne postoji — TV ce vidjeti prazan server");
    }

    let scan_options = ScanOptions {
        roots: roots.clone(),
        extensions: config.library.video_extensions.clone(),
        max_depth: config.library.max_depth,
    };
    let catalog = if args.no_scan { Default::default() } else { scan(&scan_options) };
    let counts = catalog.counts();
    info!(
        items = catalog.len(),
        videos = counts.get("videos").copied().unwrap_or(0),
        folders = counts.get("folders").copied().unwrap_or(0),
        roots = roots.len(),
        "biblioteka skenirana"
    );

    // Profili: ugradjeni + korisnicki iz `<config_dir>/profiles` (isti `id` pregazi ugradjeni).
    let profiles_dir = if config.profiles.dir.as_os_str().is_empty() {
        config_path.parent().unwrap_or_else(|| std::path::Path::new(".")).join("profiles")
    } else {
        config.profiles.dir.clone()
    };
    let mut profiles = builtin::load();
    let custom = profiles.load_dir(&profiles_dir);
    if !custom.is_empty() {
        info!(count = custom.len(), dir = %profiles_dir.display(), "korisnicki profili");
    }
    info!(count = profiles.all().len(), "profili uredjaja pripremljeni");

    let state = AppState::new(
        Arc::new(config.clone()),
        identity.clone(),
        base_url.clone(),
        catalog,
        scan_options,
        profiles,
    )
    .with_profiles_dir(profiles_dir);

    // Metapodaci u pozadini: dok TV pregledava biblioteku, kodeci su vec procitani
    // (bez ovoga bi prvi Browse s transcodeom cekao ffprobe za svaki film).
    {
        let warm = state.clone();
        tokio::spawn(async move {
            let probed = warm.warm_media_cache(5000).await;
            if probed > 0 {
                info!(probed, "metapodaci pripremljeni");
            }
        });
    }

    let app = router(state);

    let listener = TcpListener::bind((config.server.bind.as_str(), port))
        .await
        .with_context(|| format!("bind na {}:{port}", config.server.bind))?;

    let ssdp_handle = if config.server.ssdp && !args.no_ssdp {
        let ssdp_config = SsdpConfig {
            interface: ip,
            location: format!("{base_url}/rootDesc.xml"),
            server: server_header(),
            udn: identity.udn.clone(),
            device_type: DEVICE_TYPE.to_string(),
            services: vec![SERVICE_CONTENT_DIRECTORY.to_string(), SERVICE_CONNECTION_MANAGER.to_string()],
            max_age: config.server.max_age_secs,
            boot_id: boot_id(),
            config_id: 1,
        };
        Some(start_ssdp(ssdp_config).await.context("SSDP start")?)
    } else {
        warn!("SSDP je iskljucen — TV-i nas nece naci sami");
        None
    };

    print_banner(&identity.friendly_name, &base_url, &config_path);

    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    if let Some(handle) = ssdp_handle {
        handle.stop().await;
    }
    Ok(())
}

fn resolve_ip(config: &Config) -> anyhow::Result<std::net::Ipv4Addr> {
    if let Some(configured) = config.server.advertise_ip.as_deref() {
        if let Some(std::net::IpAddr::V4(ip)) = net::parse_ip(configured) {
            return Ok(ip);
        }
        warn!(value = configured, "advertise_ip nije valjana IPv4 adresa — koristim auto detekciju");
    }
    net::primary_ipv4()
        .ok_or_else(|| anyhow!("ne mogu odrediti LAN IPv4 adresu — postavi [server].advertise_ip u configu"))
}

fn boot_id() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(1)
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
