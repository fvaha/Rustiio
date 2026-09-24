//! Dijeljeno stanje servera.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustiio_core::DeviceIdentity;
use rustiio_core::config::Config;
use rustiio_library::metadata::Enricher;
use rustiio_library::{Catalog, DurationProbe, MediaProbe, NodeKind, ScanOptions, Store, scan};
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
    /// SQLite indeks: stabilni id-evi, pretraga (FTS), watch-state.
    pub store: Arc<Store>,
    /// Dohvat postera (TMDB / Wikipedia / TVmaze / Cover Art) — jedan agent i jedan kes.
    pub enricher: Arc<Enricher>,
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

    /// Poster helper (kljucevi iz okoline, kes u `<config_dir>/art`).
    pub fn with_enricher(mut self, enricher: Enricher) -> Self {
        self.enricher = Arc::new(enricher);
        self
    }

    /// Zamijeni bazu u memoriji pravom (datoteka) i uskladi id-eve kataloga.
    pub fn with_store(mut self, store: Store) -> Self {
        self.store = Arc::new(store);
        if let Ok(mut catalog) = self.catalog.try_write() {
            let summary = crate::library::sync_catalog(&self.store, &mut catalog);
            info!(roots = summary.roots, remapped = summary.remapped, "katalog prebacen na id-eve iz baze");
        }
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
        // Baza u memoriji dok `with_store` ne preda pravu datoteku. Id-evi se
        // dodjeljuju odmah (isti kod kao za datoteku) da `/api/search` radi i bez nje.
        let store = Arc::new(Store::open_memory().expect("SQLite u memoriji"));
        let mut catalog = catalog;
        crate::library::sync_catalog(&store, &mut catalog);

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
            store,
            enricher: Arc::new(Enricher::from_env(PathBuf::from("art"))),
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
        let mut fresh = scan(&self.scan_options);
        let summary = crate::library::sync_catalog(&self.store, &mut fresh);
        let count = fresh.len();
        let update_id = {
            let mut catalog = self.catalog.write().await;
            let next_update_id = catalog.update_id.wrapping_add(1).max(fresh.update_id);
            *catalog = fresh;
            catalog.update_id = next_update_id;
            next_update_id
        };
        info!(
            added = summary.added,
            updated = summary.updated,
            removed = summary.removed,
            "sken upisan u bazu"
        );
        self.duration_probe.clear();
        self.media_probe.clear();
        self.warm_probe_from_db();
        self.notify_all(update_id).await;
        count
    }

    /// Napuni `MediaProbe` cache iz baze (nakon restarta nema ponovnog ffprobe-a).
    pub fn warm_probe_from_db(&self) -> usize {
        crate::library::warm_probe_cache(&self.store, &self.media_probe)
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
                let store = self.store.clone();
                let path = path.clone();
                set.spawn_blocking(move || {
                    probe.probe(&path);
                    // Metapodatke odmah zapisi u bazu: sljedeci start ih ne mjeri ponovno.
                    if let Some(info) = probe.get(&path)
                        && let Ok(Some(item_id)) = store.item_id(&path)
                    {
                        let _ =
                            rustiio_library::store::items::update_media(&store, item_id, &info, Store::now());
                    }
                });
            }
            while set.join_next().await.is_some() {}
        }
        pending
    }
}

/// Jedan prolaz obogacivanja: poster za objekte koji ga jos nemaju.
///
/// Blokira (mreza + disk), pa ga zove pozadinski zadatak; vraca koliko je
/// objekata obradjeno.
///
/// `mark_missing` je namjerno odvojeno: u pozadinskom prolazu (pokretanje
/// servera) **ne** pisemo "nema ga" — bez mreze bi cijela biblioteka bila
/// oznacena kao gotova zauvijek. Rucni `/api/posters/refresh` smije.
pub fn enrich_posters(state: &AppState, limit: usize, mark_missing: bool) -> usize {
    let pending = match rustiio_library::store::items::items_needing_poster(&state.store, limit) {
        Ok(pending) => pending,
        Err(error) => {
            warn!(%error, "ne mogu citati objekte bez postera");
            return 0;
        }
    };
    let pending_len = pending.len();
    let mut found = 0;
    for (id, path) in pending {
        let poster = state.enricher.poster_for(id, &path);
        let (file, source) = match &poster {
            Some(poster) => (
                poster.path.file_name().map(|name| name.to_string_lossy().to_string()),
                poster.source.map(|source| source.as_str().to_string()),
            ),
            None if mark_missing => (None, Some("none".to_string())),
            None => continue,
        };
        if let Err(error) =
            rustiio_library::store::items::update_poster(&state.store, id, file.as_deref(), source.as_deref())
        {
            warn!(id, %error, "ne mogu upisati poster");
            continue;
        }
        if poster.is_some() {
            found += 1;
        }
    }
    if found > 0 {
        info!(found, processed = pending_len, "posteri dohvaceni");
    }
    pending_len
}
