//! Dizanje servera u jednom pozivu.
//!
//! Isti posao radi i `rustiio run` (CLI) i desktop aplikacija — da se ne raziđu,
//! redoslijed (config → identitet → katalog → baza → profili → SSDP → pozadinski
//! poslovi) živi samo ovdje.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use rustiio_core::config::Config;
use rustiio_core::net;
use rustiio_core::{
    DEVICE_TYPE, DeviceIdentity, SERVICE_CONNECTION_MANAGER, SERVICE_CONTENT_DIRECTORY, server_header,
};
use rustiio_library::{LibraryWatcher, ScanOptions, Store, scan};
use rustiio_profiles::builtin;
use rustiio_ssdp::{SsdpConfig, SsdpHandle, start as start_ssdp};
use tokio::net::TcpListener;
use tracing::{info, warn};

use crate::state::AppState;

/// Što pokrenuti uz server (CLI prekidači, desktop ima svoje).
#[derive(Debug, Clone, Copy)]
pub struct BootOptions {
    /// Skeniraj biblioteku odmah (inače se učitava samo baza).
    pub scan: bool,
    /// Oglašavaj se preko SSDP-a (TV-i nas tako nalaze).
    pub ssdp: bool,
    /// Prati mape i reskeniraj na promjenu.
    pub watch: bool,
    /// Dohvaćaj naslovnice u pozadini.
    pub posters: bool,
    /// Pripremaj metapodatke (ffprobe) u pozadini.
    pub probe: bool,
    /// Port; `None` znači iz configa (`server.http_port`).
    pub port: Option<u16>,
}

impl Default for BootOptions {
    fn default() -> Self {
        Self { scan: true, ssdp: true, watch: true, posters: true, probe: true, port: None }
    }
}

impl BootOptions {
    /// Kao default, ali bez SSDP-a i praćenja mapa — za alate i testove.
    pub fn offline() -> Self {
        Self { scan: false, ssdp: false, watch: false, posters: false, probe: false, port: None }
    }
}

/// Dignut server: stanje, adresa i ručke koje moraju ostati žive.
pub struct Booted {
    pub state: AppState,
    pub config: Config,
    pub identity: DeviceIdentity,
    pub ip: Ipv4Addr,
    pub port: u16,
    /// Npr. `http://10.0.0.10:8200` — isto što ide u DIDL `res`.
    pub base_url: String,
    pub listener: Option<TcpListener>,
    pub addr: SocketAddr,
    ssdp: Option<SsdpHandle>,
    /// Kad je ukljucen HTTPS, javni port drzi `front` sloj, a axum slusa na
    /// ovom lokalnom portu (HTTP za DLNA put).
    front_http: Option<u16>,
    /// Drop zaustavlja praćenje mapa, pa ga držimo uz server.
    watcher: Option<LibraryWatcher>,
}

impl Booted {
    /// Adresa na kojoj server sluša (nakon `boot`).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Poslužuj do prekida: `shutdown` se dovrši kad želimo stati (Ctrl+C, signal,
    /// ili kanal iz desktop aplikacije). Na kraju ugasi SSDP i pusti mape.
    pub async fn serve(
        mut self,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        let Some(listener) = self.listener.take() else {
            anyhow::bail!("server je već poslužen");
        };
        // Rustls bez odabranog kripto providera paničari; `ring` je dovoljan.
        let _ = rustls::crypto::ring::default_provider().install_default();
        // Web sučelje i na HTTPS-u: preglednik na HTTP-u piše "not secure".
        // Televizor ostaje na HTTP-u — DLNA ne zna za TLS.
        if let (Some(port), Some(cert), Some(key)) = (
            self.state.config.server.https_port,
            self.state.config.server.tls_cert.clone(),
            self.state.config.server.tls_key.clone(),
        ) {
            let adresa = format!("{}:{}", self.state.config.server.bind, port);
            match axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert, &key).await {
                Ok(tls) => match adresa.parse() {
                    Ok(adresa) => {
                        let app = crate::router(self.state.clone());
                        tokio::spawn(async move {
                            if let Err(greska) = axum_server::bind_rustls(adresa, tls)
                                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                                .await
                            {
                                tracing::warn!(%greska, "HTTPS server je stao");
                            }
                        });
                        tracing::info!(port, "web sučelje i na HTTPS-u");
                    }
                    Err(greska) => tracing::warn!(%greska, %adresa, "HTTPS adresa nije valjana"),
                },
                Err(greska) => tracing::warn!(
                    %greska,
                    %cert,
                    "TLS certifikat se ne može pročitati — ostajem samo na HTTP-u"
                ),
            }
        }

        // Javni port prima i HTTP (televizor) i HTTPS (preglednik): front pogleda
        // prvi bajt, pa TLS ide na HTTPS slusalicu, a sve ostalo na ovu HTTP.
        if let (Some(unutrasnji), Some(https)) = (self.front_http, self.state.config.server.https_port) {
            let bind = self.state.config.server.bind.clone();
            let javni = self.state.config.server.http_port;
            tokio::spawn(async move {
                if let Err(greska) = crate::front::slusaj(&bind, javni, unutrasnji, https).await {
                    tracing::warn!(%greska, javni, "front za HTTP/HTTPS je stao");
                }
            });
        }
        let app = crate::router(self.state.clone());
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(shutdown)
            .await?;
        self.shutdown().await;
        Ok(())
    }

    /// Ugasi SSDP (pošalje byebye) i pusti mape.
    pub async fn shutdown(mut self) {
        if let Some(handle) = self.ssdp.take() {
            handle.stop().await;
        }
        drop(self.watcher.take());
    }
}

/// Digni server: config, katalog, baza, profili, SSDP i pozadinski poslovi.
pub async fn boot(config_path: PathBuf, options: BootOptions) -> anyhow::Result<Booted> {
    let mut config = Config::load_or_create(&config_path).context("ucitavanje configa")?;
    let identity = DeviceIdentity::ensure(&mut config, &config_path)?;
    let ip = resolve_ip(&config)?;
    // Snimak **s diska** neposredno prije razrješavanja alata: po njemu sučelje
    // računa „čeka restart". Bez toga razriješena putanja ffmpeg-a vječno
    // izgleda kao promjena koju je korisnik napravio.
    let boot_config = config.clone();
    // Prvo alati: prilozeni uz binarni fajl imaju prednost pred PATH-om.
    let (ffmpeg, ffprobe) = config.resolve_tools();
    tracing::debug!(%ffmpeg, %ffprobe, "alati razrijeseni");

    let port = options.port.unwrap_or(config.server.http_port);
    let base_url = format!("http://{ip}:{port}");

    let roots = config.existing_roots();
    if roots.is_empty() {
        warn!("nijedna medijska mapa iz [library].roots ne postoji — TV ce vidjeti prazan server");
    }

    let scan_options = ScanOptions {
        roots: roots.clone(),
        extensions: config.library.video_extensions.clone(),
        max_depth: config.library.max_depth,
    };
    let catalog = if options.scan { scan(&scan_options) } else { Default::default() };
    log_catalog(&catalog, roots.len());

    let profiles_dir = profiles_dir(&config, &config_path);
    let mut profiles = builtin::load();
    let custom = profiles.load_dir(&profiles_dir);
    if !custom.is_empty() {
        info!(count = custom.len(), dir = %profiles_dir.display(), "korisnicki profili");
    }
    info!(count = profiles.all().len(), "profili uredjaja pripremljeni");

    let state = AppState::new(
        Arc::new(config.clone()),
        identity.clone(),
        base_url.clone(),
        catalog,
        scan_options,
        profiles,
    )
    .with_profiles_dir(profiles_dir)
    .with_boot_config(boot_config)
    .with_store(open_store(&config_path)?)
    .with_config_path(config_path.clone())
    // Poster kes ide uz config (`<config_dir>/art`), a kljuc (ako ga ima) iz okoline.
    .with_enricher(rustiio_library::metadata::Enricher::from_env(
        config_path.parent().unwrap_or_else(|| Path::new(".")).join("art"),
        &config.network.ip_family,
    ));

    start_probe(&state, options.probe);
    start_posters(&state, options.posters);
    crate::state::start_auto_scan(&state);
    let watcher = start_watch(&state, &config, options.watch);

    // SSDP: bez njega nas TV ne nalazi sam.
    let deljeni_port =
        config.server.front_shared_port && config.server.https_port.is_some() && options.port.is_none();
    let listener = if deljeni_port {
        TcpListener::bind(("127.0.0.1", 0)).await.with_context(|| "bind na lokalni port")?
    } else {
        TcpListener::bind((config.server.bind.as_str(), port))
            .await
            .with_context(|| format!("bind na {}:{port}", config.server.bind))?
    };
    let unutrasnji_http = listener.local_addr().with_context(|| "lokalna adresa")?.port();
    // Javna adresa ostaje ista (u nju idu DIDL `res` URL-ovi i SSDP LOCATION).
    let addr: SocketAddr = format!("{ip}:{port}")
        .parse()
        .unwrap_or_else(|_| listener.local_addr().unwrap_or_else(|_| "0.0.0.0:0".parse().expect("adresa")));
    // SSDP je važan, ali ako padne (npr. zauzet port 1900) server i dalje radi —
    // bolje reći pa nastaviti nego da se cijela aplikacija ne digne.
    let ssdp = match start_alive(&config, &identity, &ip, &base_url, options.ssdp).await {
        Ok(handle) => handle,
        Err(error) => {
            warn!(error = %error, "SSDP nije pokrenut — TV-i nas nece naci sami");
            None
        }
    };

    Ok(Booted {
        state,
        config,
        identity,
        ip,
        port,
        base_url,
        listener: Some(listener),
        addr,
        ssdp,
        watcher,
        front_http: if deljeni_port { Some(unutrasnji_http) } else { None },
    })
}

/// Skeniraj biblioteku i javi što je nađeno.
fn log_catalog(catalog: &rustiio_library::Catalog, roots: usize) {
    let counts = catalog.counts();
    info!(
        items = catalog.len(),
        videos = counts.get("videos").copied().unwrap_or(0),
        folders = counts.get("folders").copied().unwrap_or(0),
        roots,
        "biblioteka skenirana"
    );
}

/// Mapa s korisničkim profilima uređaja (uz config, ako nije zadana druga).
fn profiles_dir(config: &Config, config_path: &Path) -> PathBuf {
    if config.profiles.dir.as_os_str().is_empty() {
        config_path.parent().unwrap_or_else(|| Path::new(".")).join("profiles")
    } else {
        config.profiles.dir.clone()
    }
}

/// Metapodaci iz baze odmah (bez ffprobe-a), ostatak u pozadini.
fn start_probe(state: &AppState, enabled: bool) {
    let from_db = state.warm_probe_from_db();
    if from_db > 0 {
        info!(from_db, "metapodaci iz baze");
    }
    if !enabled {
        return;
    }
    let warm = state.clone();
    tokio::spawn(async move {
        let probed = warm.warm_media_cache(5000).await;
        if probed > 0 {
            info!(probed, "metapodaci pripremljeni");
        }
    });
}

/// Posteri: jedan prolaz u pozadini, bez blokiranja servera. Bez mreze se
/// nista ne oznacava kao "nema ga" — sljedeće pokretanje pokusa ponovno.
fn start_posters(state: &AppState, enabled: bool) {
    if !enabled || !state.config.library.posters {
        return;
    }
    let worker = state.clone();
    tokio::spawn(async move {
        let processed =
            tokio::task::spawn_blocking(move || crate::state::enrich_posters(&worker, 25, false, 200))
                .await
                .unwrap_or(0);
        if processed == 0 {
            info!("nema videa bez postera");
        }
    });
}

/// Praćenje mapa: novo/obrisano pokreće resken bez periodičnog prelaženja diska.
fn start_watch(state: &AppState, config: &Config, enabled: bool) -> Option<LibraryWatcher> {
    if !enabled || !config.library.watch {
        info!("pracenje mapa iskljuceno (library.watch = false)");
        return None;
    }
    let candidates: Vec<PathBuf> = config.library.roots.iter().map(|root| root.path.clone()).collect();
    let watched = rustiio_library::watchable_roots(&candidates);
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<()>();
    let started = LibraryWatcher::start(&watched, rustiio_library::DEFAULT_QUIET, move || {
        let _ = sender.send(());
    });
    match started {
        Ok(watcher) => {
            let quiet = watcher.quiet();
            info!(roots = watcher.roots().len(), quiet_s = quiet.as_secs(), "pratim mape");
            let live = state.clone();
            tokio::spawn(async move {
                // Prvi događaj pokreće sken; daljnji dok se ne stiša samo produžuju čekanje.
                while receiver.recv().await.is_some() {
                    while tokio::time::timeout(quiet, receiver.recv()).await.is_ok() {}
                    let started = std::time::Instant::now();
                    let items = live.rescan().await;
                    info!(items, ms = started.elapsed().as_millis() as u64, "datoteke promijenjene — resken");
                }
            });
            Some(watcher)
        }
        Err(error) => {
            warn!(error = %error, "pracenje mapa nije pokrenuto — resken ostaje rucni");
            None
        }
    }
}

/// SSDP oglas (alive + odgovori na M-SEARCH).
async fn start_alive(
    config: &Config,
    identity: &DeviceIdentity,
    ip: &Ipv4Addr,
    base_url: &str,
    enabled: bool,
) -> anyhow::Result<Option<SsdpHandle>> {
    if !enabled || !config.server.ssdp {
        warn!("SSDP je iskljucen — TV-i nas nece naci sami");
        return Ok(None);
    }
    let ssdp_config = SsdpConfig {
        interface: *ip,
        location: format!("{base_url}/rootDesc.xml"),
        server: server_header(),
        udn: identity.udn.clone(),
        device_type: DEVICE_TYPE.to_string(),
        services: vec![SERVICE_CONTENT_DIRECTORY.to_string(), SERVICE_CONNECTION_MANAGER.to_string()],
        max_age: config.server.max_age_secs,
        boot_id: boot_id(),
        config_id: 1,
    };
    Ok(Some(start_ssdp(ssdp_config).await.context("SSDP start")?))
}

/// LAN IPv4 adresa koju ćemo reklamirati TV-ima.
fn resolve_ip(config: &Config) -> anyhow::Result<Ipv4Addr> {
    if let Some(configured) = config.server.advertise_ip.as_deref() {
        if let Some(std::net::IpAddr::V4(ip)) = net::parse_ip(configured) {
            return Ok(ip);
        }
        warn!(value = configured, "advertise_ip nije valjana IPv4 adresa — koristim auto detekciju");
    }
    net::primary_ipv4()
        .ok_or_else(|| anyhow!("ne mogu odrediti LAN IPv4 adresu — postavi [server].advertise_ip u configu"))
}

fn boot_id() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(1)
}

/// Otvori SQLite indeks biblioteke.
///
/// Putanja je `<config_dir>/rustiio.db`; `RUSTIIO_DB` je pregazi (korisno za testove
/// i za bazu na drugom disku). Baza pamti stabilne DLNA id-eve, metapodatke i
/// watch-state, pa se ne smije izgubiti između restarta.
pub fn open_store(config_path: &Path) -> anyhow::Result<Store> {
    let path = std::env::var_os("RUSTIIO_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| config_path.parent().unwrap_or_else(|| Path::new(".")).join("rustiio.db"));
    let store = Store::open(&path).with_context(|| format!("SQLite baza {}", path.display()))?;
    info!(
        path = %path.display(),
        schema = store.schema_version().unwrap_or(0),
        items = store.item_count().unwrap_or(0),
        "baza otvorena"
    );
    Ok(store)
}

/// Broj slobodnog porta iznad zadanog — desktop app tako nađe mjesto ako je
/// nešto drugo zauzelo `server.http_port`.
pub async fn free_port(from: u16) -> Option<u16> {
    for port in from..from.saturating_add(50) {
        if TcpListener::bind(("127.0.0.1", port)).await.is_ok() {
            return Some(port);
        }
    }
    None
}
