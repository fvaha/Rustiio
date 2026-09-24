//! Podnaredbe i inicijalizacija loganja.

pub mod doctor;
pub mod health;
pub mod init;
pub mod posters;
pub mod probe;
pub mod run;
pub mod service;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Loga se na stdout; `RUSTIIO_LOG` / `--log` daju razinu, `RUST_LOG` je override.
pub fn init_tracing(level: &str) {
    let default = format!(
        "warn,rustiio={level},rustiio_core={level},rustiio_ssdp={level},rustiio_upnp={level},\
         rustiio_library={level},rustiio_cds={level},rustiio_http={level},rustiio_server={level}"
    );
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()));
    // Zadrzane linije + live tok za web sucelje (`/api/logs`, `/ws/logs`).
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(rustiio_server::api::logs::BufferLayer)
        .init();
}
