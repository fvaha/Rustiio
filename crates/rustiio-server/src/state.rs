//! Dijeljeno stanje servera.

use std::sync::Arc;
use std::time::Instant;

use rustiio_core::DeviceIdentity;
use rustiio_core::config::Config;
use rustiio_library::{Catalog, ScanOptions, scan};
use rustiio_upnp::DeviceMeta;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub identity: DeviceIdentity,
    /// `http://192.168.1.10:8200` — osnova svih URL-ova u DIDL-u.
    pub base_url: Arc<String>,
    pub scan_options: Arc<ScanOptions>,
    pub catalog: Arc<RwLock<Catalog>>,
    started: Instant,
}

impl AppState {
    pub fn new(
        config: Arc<Config>,
        identity: DeviceIdentity,
        base_url: String,
        catalog: Catalog,
        scan_options: ScanOptions,
    ) -> Self {
        Self {
            config,
            identity,
            base_url: Arc::new(base_url),
            scan_options: Arc::new(scan_options),
            catalog: Arc::new(RwLock::new(catalog)),
            started: Instant::now(),
        }
    }

    pub fn device_meta(&self) -> DeviceMeta {
        DeviceMeta {
            udn: self.identity.udn.clone(),
            friendly_name: self.identity.friendly_name.clone(),
            manufacturer: self.identity.manufacturer.clone(),
            model_name: self.identity.model_name.clone(),
            model_number: self.identity.model_number.clone(),
            model_description: "Rustiio DLNA/UPnP media server".to_string(),
            serial_number: self.identity.serial_number.clone(),
            base_url: self.base_url.as_ref().clone(),
        }
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Ponovno skeniraj mape i zamijeni katalog (REST: `POST /api/rescan`).
    pub async fn rescan(&self) -> usize {
        let fresh = scan(&self.scan_options);
        let count = fresh.len();
        let mut catalog = self.catalog.write().await;
        let next_update_id = catalog.update_id.wrapping_add(1).max(fresh.update_id);
        *catalog = fresh;
        catalog.update_id = next_update_id;
        count
    }
}
