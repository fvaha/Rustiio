//! Dijeljeno stanje servera.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rustiio_core::DeviceIdentity;
use rustiio_core::config::Config;
use rustiio_library::{Catalog, DurationProbe, ScanOptions, scan};
use rustiio_upnp::DeviceMeta;
use tokio::sync::RwLock;
use tracing::warn;

use crate::gena;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub identity: DeviceIdentity,
    /// `http://192.168.1.10:8200` — osnova svih URL-ova u DIDL-u.
    pub base_url: Arc<String>,
    pub scan_options: Arc<ScanOptions>,
    pub catalog: Arc<RwLock<Catalog>>,
    /// Lijeno trajanje (ffprobe) — treba samo za `TimeSeekRange`.
    pub duration_probe: Arc<DurationProbe>,
    /// Aktivne GENA pretplate (TV-i koji cekaju evente).
    pub gena: Arc<gena::Registry>,
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
        let duration_probe = Arc::new(DurationProbe::new(
            config.transcode.ffprobe_path.clone(),
            config.transcode.probe_duration,
        ));
        Self {
            config,
            identity,
            base_url: Arc::new(base_url),
            scan_options: Arc::new(scan_options),
            catalog: Arc::new(RwLock::new(catalog)),
            duration_probe,
            gena: Arc::new(gena::Registry::new()),
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

    pub fn update_id(&self) -> u32 {
        self.catalog.try_read().map(|catalog| catalog.update_id).unwrap_or(0)
    }

    /// Ponovno skeniraj mape i zamijeni katalog (REST: `POST /api/rescan`).
    ///
    /// Nakon skena salje eventе pretplacenim uredjajima — inace TV drzi stari popis.
    pub async fn rescan(&self) -> usize {
        let fresh = scan(&self.scan_options);
        let count = fresh.len();
        let update_id = {
            let mut catalog = self.catalog.write().await;
            let next_update_id = catalog.update_id.wrapping_add(1).max(fresh.update_id);
            *catalog = fresh;
            catalog.update_id = next_update_id;
            next_update_id
        };
        self.duration_probe.clear();
        self.notify_all(update_id).await;
        count
    }

    /// Posalji evente svim pretplatama (ContentDirectory + ConnectionManager).
    pub async fn notify_all(&self, update_id: u32) -> usize {
        let bodies = [
            (gena::CONTENT_DIRECTORY, gena::content_directory_body(update_id)),
            (
                gena::CONNECTION_MANAGER,
                gena::connection_manager_body(
                    &rustiio_upnp::protocol::source_protocol_info(&self.config.library.video_extensions),
                    "",
                ),
            ),
        ];

        let mut delivered = 0;
        for (service, body) in bodies {
            for subscription in self.gena.list(service) {
                let sid = subscription.sid.clone();
                if gena::send_notify(&subscription, &body, Duration::from_secs(5)).await {
                    self.gena.bump_seq(&sid);
                    delivered += 1;
                } else {
                    warn!(sid = %sid, "pretplata nedostupna — brisem je");
                    self.gena.unsubscribe(&sid);
                }
            }
        }
        self.gena.purge_expired();
        delivered
    }
}
