//! Definicija naredbenog sučelja.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rustiio",
    version,
    about = "Rustiio — univerzalni DLNA/UPnP media server",
    long_about = "DLNA/UPnP (DMS) server u Rustu: profili uredaja, transcode po potrebi, \
                  web sučelje i desktop GUI. Pokreni bez podnaredbe za server."
)]
pub struct Cli {
    /// Putanja do config.toml (default: platformska config mapa).
    #[arg(short, long, global = true, env = "RUSTIIO_CONFIG", value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Razina loga: trace, debug, info, warn, error.
    #[arg(long, global = true, default_value = "info", env = "RUSTIIO_LOG")]
    pub log: String,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Pokreni DLNA server i web sučelje (default).
    Run(RunArgs),
    /// Nadji DLNA uredaje u mrezi (SSDP M-SEARCH).
    Probe(ProbeArgs),
    /// Provjeri okolinu: config, IP, port, ffmpeg, mape.
    Doctor,
    /// Provjeri radi li server (HTTP 200 na /healthz); izlazni kod 1 ako ne radi.
    Health(HealthArgs),
    /// Napisi default config i izadji.
    Init,
    /// Dohvati postere za videe koji ih nemaju (radi i dok server radi).
    Posters(PostersArgs),
    /// Pusti server kao uslugu (systemd / launchd / Windows servis).
    Service(ServiceArgs),
}

#[derive(Debug, Args)]
pub struct ServiceArgs {
    /// install = upiši i pokreni, uninstall = zaustavi i obriši, status = provjeri.
    #[arg(value_enum)]
    pub action: crate::cmd::service::Action,
}

impl Default for Command {
    fn default() -> Self {
        Command::Run(RunArgs::default())
    }
}

#[derive(Debug, Default, Args)]
pub struct RunArgs {
    /// Ne oglasavaj se preko SSDP-a (samo HTTP).
    #[arg(long)]
    pub no_ssdp: bool,
    /// Ne skeniraj mape pri startu (prazna biblioteka).
    #[arg(long)]
    pub no_scan: bool,
}

#[derive(Debug, Args)]
pub struct ProbeArgs {
    /// Koliko sekundi cekati odgovore.
    #[arg(short, long, default_value_t = 3)]
    pub wait: u64,
    /// Search target (npr. urn:schemas-upnp-org:device:MediaServer:1).
    #[arg(short, long, default_value = "ssdp:all")]
    pub st: String,
}

#[derive(Debug, Args)]
pub struct HealthArgs {
    /// URL koji se provjerava.
    #[arg(short, long, default_value = "http://127.0.0.1:8200/healthz")]
    pub url: String,
    /// Koliko sekundi cekati odgovor.
    #[arg(short, long, default_value_t = 3)]
    pub timeout: u64,
    /// Ne ispisuj nista kad je sve u redu (za skripte).
    #[arg(short, long)]
    pub quiet: bool,
}

#[derive(Debug, Default, Args)]
pub struct PostersArgs {
    /// Koliko objekata u jednoj turi.
    #[arg(short, long, default_value_t = 25)]
    pub batch: usize,
    /// Najvise tura (svaka tura = `batch` objekata).
    #[arg(short, long, default_value_t = 200)]
    pub max_batches: usize,
    /// Zapisi i "probano, nema ga" (bez ovoga se isti objekti probaju svaki put).
    #[arg(long)]
    pub mark_missing: bool,
    /// Zaboravi prijasnje "nema ga" pa probaj ponovno sve.
    #[arg(long)]
    pub reset: bool,
}
