//! Prozor + ožičenje: server se diže u pozadini, prozor pokazuje na njega.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod paths;
mod server;

fn main() {
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
        .on_window_event(|_window, event| {
            // Zatvaranje prozora gasi i server (SSDP byebye, praćenje mapa).
            if matches!(event, tauri::WindowEvent::Destroyed) {
                server::stop();
            }
        })
        .run(tauri::generate_context!())
        .expect("Rustiio: prozor se nije pokrenuo");
}
