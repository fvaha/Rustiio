//! Linux: systemd — korisnička usluga kad se ne vrtimo kao root, inače sustavska.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

use super::{Action, report, server_args};

pub const UNIT: &str = "rustiio.service";

/// Je li proces pokrenut kao root (`id -u` == 0).
fn is_root() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim() == "0")
        .unwrap_or(false)
}

/// Sustavska jedinica (`/etc/systemd/system`) ili korisnička (`~/.config/systemd/user`).
pub fn unit_path() -> anyhow::Result<PathBuf> {
    if is_root() {
        return Ok(PathBuf::from("/etc/systemd/system").join(UNIT));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).context("nema HOME u okolini")?;
    Ok(home.join(".config/systemd/user").join(UNIT))
}

/// Sadržaj systemd jedinice.
pub fn unit(exe: &Path, config_path: &Path, user_service: bool) -> String {
    let args = server_args(exe, config_path);
    let exec = args.iter().map(|arg| quote(arg)).collect::<Vec<_>>().join(" ");
    let wanted = if user_service { "WantedBy=default.target" } else { "WantedBy=multi-user.target" };
    format!(
        r#"[Unit]
Description=Rustiio — DLNA/UPnP media server
Documentation=https://vaha.net
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={exec}
Restart=on-failure
RestartSec=3
# Server radi kao obican korisnik; drzi ga zivim systemd, ne shell.
KillSignal=SIGTERM
TimeoutStopSec=10

[Install]
{wanted}
"#
    )
}

/// Systemd traži navodnike oko argumenata s razmacima.
fn quote(arg: &str) -> String {
    if arg.contains(' ') || arg.contains('"') {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

fn systemctl(user_service: bool, args: &[&str]) -> anyhow::Result<bool> {
    let mut full: Vec<String> = vec!["systemctl".to_string()];
    if user_service {
        full.push("--user".to_string());
    }
    full.extend(args.iter().map(|arg| arg.to_string()));
    let status = Command::new(&full[0])
        .args(&full[1..])
        .status()
        .with_context(|| format!("pokretanje {}", full.join(" ")))?;
    Ok(status.success())
}

fn is_active(user_service: bool) -> bool {
    systemctl(user_service, &["is-active", "--quiet", "rustiio"]).unwrap_or(false)
}

pub fn run(action: Action, exe: &Path, config_path: &Path) -> anyhow::Result<()> {
    let user_service = !is_root();
    let file = unit_path()?;
    match action {
        Action::Install => {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).with_context(|| format!("mapa {}", dir.display()))?;
            }
            std::fs::write(&file, unit(exe, config_path, user_service))
                .with_context(|| format!("pisanje {}", file.display()))?;
            // Bez `daemon-reload` systemd ne vidi novu jedinicu.
            let _ = systemctl(user_service, &["daemon-reload"]);
            let started = systemctl(user_service, &["enable", "--now", "rustiio"])?;
            if !started {
                bail!(
                    "jedinica je upisana, ali je systemd nije pokrenuo — provjeri: systemctl {} status rustiio",
                    if user_service { "--user" } else { "" }
                );
            }
            report(
                "systemd",
                &file,
                is_active(user_service),
                if user_service {
                    "korisnicka usluga (bez roota) — dnevnik: journalctl --user -u rustiio -f"
                } else {
                    "sustavska usluga — dnevnik: journalctl -u rustiio -f"
                },
            );
        }
        Action::Uninstall => {
            let _ = systemctl(user_service, &["disable", "--now", "rustiio"]);
            if file.exists() {
                std::fs::remove_file(&file).with_context(|| format!("brisanje {}", file.display()))?;
            }
            let _ = systemctl(user_service, &["daemon-reload"]);
            report("systemd", &file, false, "usluga je zaustavljena i obrisana");
        }
        Action::Status => {
            report(
                "systemd",
                &file,
                is_active(user_service),
                if user_service {
                    "korisnicka usluga — journalctl --user -u rustiio -f"
                } else {
                    "sustavska usluga — journalctl -u rustiio -f"
                },
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_has_exec_start_and_install_target() {
        let text =
            unit(Path::new("/usr/local/bin/rustiio"), Path::new("/var/lib/rustiio/config.toml"), false);
        assert!(text.contains("ExecStart=/usr/local/bin/rustiio run --config /var/lib/rustiio/config.toml"));
        assert!(text.contains("WantedBy=multi-user.target"));
        assert!(text.contains("Restart=on-failure"));
    }

    #[test]
    fn user_unit_targets_default() {
        let text =
            unit(Path::new("/home/user/rustiio"), Path::new("/home/user/.config/rustiio/config.toml"), true);
        assert!(text.contains("WantedBy=default.target"), "korisnicka jedinica: {text}");
    }

    #[test]
    fn paths_with_spaces_are_quoted() {
        let text = unit(Path::new("/opt/Moj Rustiio/rustiio"), Path::new("/tmp/c.toml"), true);
        assert!(text.contains("ExecStart=\"/opt/Moj Rustiio/rustiio\" run --config /tmp/c.toml"), "{text}");
    }
}
