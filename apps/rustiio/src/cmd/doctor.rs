//! `rustiio doctor` — provjera okoline prije nego sto se pozalis da TV ne vidi server.

use std::path::{Path, PathBuf};

use rustiio_core::config::Config;
use rustiio_core::net;
use rustiio_library::{ScanOptions, scan};

pub fn execute(config_path: PathBuf) -> anyhow::Result<()> {
    println!("\n  {} v{} — doctor\n", rustiio_core::APP_NAME, rustiio_core::VERSION);

    // 1. Config
    println!("  config:        {}", config_path.display());
    println!("  postoji:       {}", if config_path.exists() { "da" } else { "ne (pokreni `rustiio init`)" });

    let mut config = Config::load_or_default(&config_path).unwrap_or_default();

    // 2. Identitet
    match rustiio_core::DeviceIdentity::ensure(&mut config, Path::new("")) {
        Ok(identity) => {
            println!("  uređaj:        {}", identity.friendly_name);
            println!("  UDN:           {}", identity.udn);
        }
        Err(err) => println!("  UDN:           GRESKA ({err})"),
    }

    // 3. Mreza
    match net::primary_ipv4() {
        Some(ip) => println!("  LAN IP:        {ip}"),
        None => println!("  LAN IP:        nije detektiran (postavi [server].advertise_ip)"),
    }
    let port = config.server.http_port;
    let bind = format!("{}:{port}", config.server.bind);
    match std::net::TcpListener::bind(&bind) {
        Ok(_) => println!("  HTTP port:     {bind} — slobodan"),
        Err(err) => println!("  HTTP port:     {bind} — ZAUZET ({err})"),
    }
    println!("  SSDP:          {}", if config.server.ssdp { "ukljucen (UDP 1900)" } else { "iskljucen" });

    // 4. Mape i biblioteka
    let roots = config.existing_roots();
    println!("\n  mape ({} postojećih od {}):", roots.len(), config.library.roots.len());
    for root in &config.library.roots {
        let mark = if root.path.is_dir() { "ok " } else { "ne " };
        println!("    [{mark}] {} — {}", root.label, root.path.display());
    }

    if !roots.is_empty() {
        let started = std::time::Instant::now();
        let catalog = scan(&ScanOptions {
            roots: roots.clone(),
            extensions: config.library.video_extensions.clone(),
            max_depth: config.library.max_depth,
        });
        let counts = catalog.counts();
        println!(
            "\n  biblioteka:    {} objekata ({} video, {} mapa) skenirano za {:.0} ms",
            catalog.len(),
            counts.get("videos").copied().unwrap_or(0),
            counts.get("folders").copied().unwrap_or(0),
            started.elapsed().as_secs_f64() * 1000.0
        );
    }

    // 5. ffmpeg / ffprobe (potrebni tek u Fazi 2, ali bolje znati odmah)
    println!("\n  vanjski alati:");
    check_tool(&config.transcode.ffmpeg_path, "--version");
    check_tool(&config.transcode.ffprobe_path, "-version");
    println!(
        "\n  transcode:     {} (hw accel: {})",
        if config.transcode.enabled { "ukljucen" } else { "iskljucen" },
        config.transcode.hw_accel
    );

    println!();
    Ok(())
}

fn check_tool(program: &str, version_flag: &str) {
    let output = std::process::Command::new(program).arg(version_flag).output();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let first = text.lines().next().unwrap_or("").trim();
            println!("    [ok ] {program} — {first}");
        }
        Ok(_) => println!("    [ne ] {program} — postoji, ali se ne pokrece ispravno"),
        Err(_) => println!("    [ne ] {program} — nije u PATH-u"),
    }
}
