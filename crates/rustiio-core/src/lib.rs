//! Rustiio temelj: konfiguracija, identitet uredaja, mrezni pomocnici i vrijeme.

pub mod config;
pub mod device;
pub mod net;
pub mod time;

pub use config::Config;
pub use device::DeviceIdentity;

/// Ime aplikacije kako se pojavljuje u `SERVER` headeru i logovima.
pub const APP_NAME: &str = "Rustiio";
/// Verzija iz Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// UPnP spec verzija koju oglasavamo.
pub const UPNP_VERSION: &str = "1.0";

/// Tip uredaja koji oglasavamo (DLNA DMS).
pub const DEVICE_TYPE: &str = "urn:schemas-upnp-org:device:MediaServer:1";
pub const SERVICE_CONTENT_DIRECTORY: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";
pub const SERVICE_CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";

/// Default HTTP port (Serviio na .10 drzi 8895, pa nema sudara).
pub const DEFAULT_HTTP_PORT: u16 = 8200;
/// Default SSDP max-age (sekunde).
pub const DEFAULT_MAX_AGE: u32 = 1800;

/// `SERVER` header u SSDP/UPnP odgovorima.
pub fn server_header() -> String {
    format!("{} UPnP/1.0 {}/{}", os_name(), APP_NAME, VERSION)
}

fn os_name() -> String {
    format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH)
}
