//! Windows: pravi servis preko `sc.exe` (traži povišene ovlasti).

use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};

use super::{Action, report, server_args};

pub const NAME: &str = "Rustiio";

/// `binPath=` za `sc create` (jedan string, kako `sc` očekuje).
pub fn bin_path(exe: &Path, config_path: &Path) -> String {
    let args = server_args(exe, config_path);
    // `sc` prima cijelu naredbu kao jedan argument; putanje s razmacima idu pod navodnike.
    format!("\"{}\"", args.join(" "))
}

fn sc(args: &[String]) -> anyhow::Result<bool> {
    let mut full: Vec<String> = vec!["sc.exe".to_string()];
    full.extend(args.iter().cloned());
    let status = Command::new(&full[0])
        .args(&full[1..])
        .status()
        .with_context(|| format!("pokretanje {}", full.join(" ")))?;
    Ok(status.success())
}

fn is_installed() -> bool {
    Command::new("sc.exe").arg("query").arg(NAME).output().map(|out| out.status.success()).unwrap_or(false)
}

pub fn run(action: Action, exe: &Path, config_path: &Path) -> anyhow::Result<()> {
    let file = Path::new("(Windows servis — registar)");
    match action {
        Action::Install => {
            if !is_installed() {
                let created = sc(&[
                    "create".to_string(),
                    NAME.to_string(),
                    format!("binPath= {}", bin_path(exe, config_path)),
                    "start= auto".to_string(),
                    "DisplayName= Rustiio media server".to_string(),
                ])?;
                if !created {
                    bail!("`sc create` nije prosao — otvori PowerShell kao administrator i ponovi");
                }
            }
            let started = sc(&["start".to_string(), NAME.to_string()])?;
            report(
                "Windows servis",
                file,
                is_installed(),
                if started { "servis je pokrenut" } else { "servis je upisan; pokreni ga: sc start Rustiio" },
            );
        }
        Action::Uninstall => {
            let _ = sc(&["stop".to_string(), NAME.to_string()]);
            let deleted = sc(&["delete".to_string(), NAME.to_string()])?;
            report(
                "Windows servis",
                file,
                false,
                if deleted { "servis je obrisan" } else { "servis nije bio upisan" },
            );
        }
        Action::Status => {
            report("Windows servis", file, is_installed(), "provjera: sc query Rustiio");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_path_quotes_the_whole_command() {
        let value = bin_path(
            Path::new("C:\\Program Files\\Rustiio\\rustiio.exe"),
            Path::new("C:\\Rustiio\\config.toml"),
        );
        assert!(value.starts_with('"') && value.ends_with('"'), "{value}");
        assert!(value.contains("run --config C:\\Rustiio\\config.toml"), "{value}");
    }
}
