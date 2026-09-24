//! Dijeljeno stanje servera.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustiio_core::DeviceIdentity;
use rustiio_core::config::Config;
use rustiio_library::{Catalog, DurationProbe, MediaProbe, NodeKind, ScanOptions, scan};
use rustiio_profiles::{Capture, ProfileSet};
use rustiio_transcode::{HwSupport, SessionManager, detect_hw};
use rustiio_upnp::DeviceMeta;
use tokio::sync::RwLock;
use tracing::{info, warn};

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
    /// Metapodaci (kodek, rezolucija, kanali) — treba decision engineu.
    pub media_probe: Arc<MediaProbe>,
    /// Aktivne GENA pretplate (TV-i koji cekaju evente).
    pub gena: Arc<gena::Registry>,
    /// Profili uredjaja (ugradjeni + korisnicki iz `<config_dir>/profiles`).
    pub profiles: Arc<RwLock<ProfileSet>>,
    /// Zapisi stvarnih uredjaja (User-Agent, DLNA zaglavlja) — osnova za nove profile.
    pub capture: Arc<Capture>,
    /// Transcode sesije (ffmpeg) s limitom paralelnih.
    pub sessions: Arc<SessionManager>,
    /// Mapa s korisnickim profilima.
    profiles_dir: Arc<PathBuf>,
    started: Instant,
}

impl AppState {
    /// Isti kao [`AppState::new`], ali s izricitom mapom profila.
    pub fn with_profiles_dir(mut self, dir: PathBuf) -> Self {
        self.profiles_dir = Arc::new(dir);
        self
    }

    pub fn new(
        config: Arc<Config>,
        identity: DeviceIdentity,
        base_url: String,
        catalog: Catalog,
        scan_options: ScanOptions,
        profiles: ProfileSet,
    ) -> Self {
        let duration_probe = Arc::new(DurationProbe::new(
            config.transcode.ffprobe_path.clone(),
            config.transcode.probe_duration,
        ));
        let media_probe = Arc::new(MediaProbe::new(
            config.transcode.ffprobe_path.clone(),
            config.transcode.probe_duration || config.transcode.enabled,
        ));
        // Detekcija HW ubrzanja je testno enkodiranje — samo ako transcode radi.
        let hw = if config.transcode.enabled {
            detect_hw(&config.transcode.ffmpeg_path, &config.transcode.hw_accel)
        } else {
            HwSupport::software()
        };
        if config.transcode.enabled {
            info!(
                hw = %hw.summary(),
                encoders = ?hw.available.iter().map(|hw| hw.name()).collect::<Vec<_>>(),
                "transcode pripremljen"
            );
        }
        let sessions = Arc::new(SessionManager::new(
            config.transcode.ffmpeg_path.clone(),
            hw,
            config.transcode.max_concurrent,
        ));

        Self {
            config,
            identity,
            base_url: Arc::new(base_url),
            scan_options: Arc::new(scan_options),
            catalog: Arc::new(RwLock::new(catalog)),
            duration_probe,
            media_probe,
            gena: Arc::new(gena::Registry::new()),
            profiles: Arc::new(RwLock::new(profiles)),
            capture: Arc::new(Capture::new()),
            sessions,
            profiles_dir: Arc::new(PathBuf::from("profiles")),
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
    /// Nakon skena salje evente pretplacenim uredjajima — inace TV drzi stari popis.
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
        self.media_probe.clear();
        self.notify_all(update_id).await;
        count
    }

    /// Ponovno ucitaj profile s diska (REST: `POST /api/profiles/reload`).
    pub async fn reload_profiles(&self) -> usize {
        let mut fresh = rustiio_profiles::builtin::load();
        let dir = self.profiles_dir();
        let loaded = fresh.load_dir(&dir);
        if !loaded.is_empty() {
            info!(count = loaded.len(), dir = %dir.display(), "korisnicki profili ucitani");
        }
        let mut profiles = self.profiles.write().await;
        *profiles = fresh;
        profiles.all().len()
    }

    /// Mapa s korisnickim profilima (`<config_dir>/profiles` ako nije zadano).
    pub fn profiles_dir(&self) -> PathBuf {
        self.profiles_dir.as_ref().clone()
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

    /// Ucitaj metapodatke za fajlove koji jos nisu u cacheu (pozadinski "warm up").
    ///
    /// Bez ovoga bi prvi Browse s transcodeom cekao ffprobe za svaki fajl; s ovim se
    /// cache puni nakon skena, pa je odluka trenutna.
    pub async fn warm_media_cache(&self, limit: usize) -> usize {
        if !self.config.transcode.enabled {
            return 0;
        }
        let paths: Vec<PathBuf> = {
            let catalog = self.catalog.read().await;
            let mut seen = HashSet::new();
            catalog
                .of_kinds(&[NodeKind::Video, NodeKind::Audio])
                .into_iter()
                .filter(|node| !self.media_probe.is_known(&node.path))
                .filter(|node| seen.insert(node.path.clone()))
                .map(|node| node.path)
                .take(limit)
                .collect()
        };
        let pending = paths.len();
        for chunk in paths.chunks(4) {
            let mut set = tokio::task::JoinSet::new();
            for path in chunk {
                let probe = self.media_probe.clone();
                let path = path.clone();
                set.spawn_blocking(move || {
                    probe.probe(&path);
                });
            }
            while set.join_next().await.is_some() {}
        }
        pending
    }
}
