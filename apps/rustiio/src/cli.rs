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
    /// Napisi default config i izadji.
    Init,
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
