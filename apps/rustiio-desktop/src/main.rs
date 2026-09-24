//! Prozor + ožičenje: server se diže u pozadini, prozor pokazuje na njega.

/// Dnevnik: razina iz `RUSTIIO_LOG` (default `info`).
///
/// Desktop aplikacija inače ne bi ostavljala nikakav trag — kad je digne
/// `launchd`/`systemd`, ovaj izlaz ide u dnevnik te usluge.
fn init_logging() {
    let level = std::env::var("RUSTIIO_LOG").unwrap_or_else(|_| "info".to_string());
    let _ = tracing_subscriber::fmt().with_env_filter(level).try_init();
}

#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod paths;
mod server;

use tauri::Manager;

fn main() {
    init_logging();
    // Server prvo: prozor ima što pokazati čim se otvori.
    let config_path = match paths::config_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Rustiio: ne mogu odrediti mapu s configom: {error}");
            std::process::exit(1);
        }
    };
    let url = match server::spawn(config_path) {
        Ok(url) => url,
        Err(error) => {
            eprintln!("Rustiio: {error}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .setup(move |app| {
            let address = url.parse()?;
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(address))
                .title("Rustiio")
                .inner_size(1280.0, 840.0)
                .min_inner_size(900.0, 600.0)
                .build()?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Zatvaranje prozora gasi i server (SSDP byebye, praćenje mapa) i samu
            // aplikaciju — inače Tauri ostane živ bez prozora, a server drži port.
            if matches!(event, tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed) {
                server::stop();
                window.app_handle().exit(0);
            }
        })
        .run(tauri::generate_context!())
        .expect("Rustiio: prozor se nije pokrenuo");
}
