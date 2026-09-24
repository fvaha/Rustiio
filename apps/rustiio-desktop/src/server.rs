//! Server u pozadinskom threadu: desktop app ne treba zaseban proces ni port
//! koji je netko drugi zauzeo.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rustiio_core::config::Config;
use rustiio_server::BootOptions;

/// Prekid servera (drop kanala gasi `serve`, pa SSDP pošalje byebye).
static STOP: Mutex<Option<tokio::sync::oneshot::Sender<()>>> = Mutex::new(None);

/// Digne server i vrati URL koji je spreman za prozor.
///
/// Blokira dok server ne odgovori (prvi sken zna potrajati) ili dok ne istekne 60 s.
pub fn spawn(config_path: PathBuf) -> anyhow::Result<String> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<String, String>>();
    std::thread::Builder::new().name("rustiio-server".to_string()).spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = ready_tx.send(Err(error.to_string()));
                return;
            }
        };
        runtime.block_on(async move {
            let options =
                rustiio_server::BootOptions { port: port_for(&config_path).await, ..BootOptions::default() };
            match rustiio_server::boot(config_path, options).await {
                Ok(booted) => {
                    let url = format!("{}/", booted.base_url);
                    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
                    if let Ok(mut slot) = STOP.lock() {
                        *slot = Some(stop_tx);
                    }
                    let _ = ready_tx.send(Ok(url));
                    let _ = booted
                        .serve(async move {
                            let _ = stop_rx.await;
                        })
                        .await;
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                }
            }
        });
    })?;

    match ready_rx.recv_timeout(Duration::from_secs(60)) {
        Ok(Ok(url)) => Ok(url),
        Ok(Err(error)) => anyhow::bail!("server nije dignut: {error}"),
        Err(_) => anyhow::bail!("server se nije javio u 60 s"),
    }
}

/// Ugasi server (prozor je zatvoren).
pub fn stop() {
    if let Ok(mut slot) = STOP.lock() {
        if let Some(sender) = slot.take() {
            let _ = sender.send(());
        }
    }
}

/// Port iz configa; ako je zauzet, prvi slobodan iznad njega.
async fn port_for(config_path: &Path) -> Option<u16> {
    let wanted = Config::load_or_create(config_path).map(|config| config.server.http_port).unwrap_or(8200);
    if tokio::net::TcpListener::bind(("0.0.0.0", wanted)).await.is_ok() {
        return Some(wanted);
    }
    rustiio_server::boot::free_port(wanted + 1).await
}
