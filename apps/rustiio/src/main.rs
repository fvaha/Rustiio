//! `rustiio` — jedan binarni fajl: server, probe i dijagnostika.

mod cli;
mod cmd;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    cmd::init_tracing(&cli.log);
    let config_path = cli.config.clone().unwrap_or_else(rustiio_core::config::config_path);

    match cli.command.unwrap_or_default() {
        Command::Run(args) => cmd::run::execute(config_path, args).await,
        Command::Probe(args) => cmd::probe::execute(args).await,
        Command::Doctor => cmd::doctor::execute(config_path),
        Command::Health(args) => cmd::health::execute(args).await,
        Command::Init => cmd::init::execute(config_path),
    }
}
