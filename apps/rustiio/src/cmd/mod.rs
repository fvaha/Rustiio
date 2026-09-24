//! Podnaredbe i inicijalizacija loganja.

pub mod doctor;
pub mod health;
pub mod init;
pub mod posters;
pub mod probe;
pub mod run;

use tracing_subscriber::EnvFilter;

/// Loga se na stdout; `RUSTIIO_LOG` / `--log` daju razinu, `RUST_LOG` je override.
pub fn init_tracing(level: &str) {
    let default = format!(
        "warn,rustiio={level},rustiio_core={level},rustiio_ssdp={level},rustiio_upnp={level},\
         rustiio_library={level},rustiio_cds={level},rustiio_http={level},rustiio_server={level}"
    );
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();
}
