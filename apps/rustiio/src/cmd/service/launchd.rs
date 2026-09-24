//! macOS: korisnički launchd agent — diže se pri prijavi, bez roota.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

use super::{Action, report, server_args};

/// Oznaka usluge (mora biti ista u imenu datoteke i u plistu).
pub const LABEL: &str = "net.vaha.rustiio";

/// `~/Library/LaunchAgents/net.vaha.rustiio.plist`
pub fn plist_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).context("nema HOME u okolini")?;
    Ok(home.join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

/// Sadržaj plista za zadani program i config.
pub fn plist(exe: &Path, config_path: &Path, log: &Path) -> String {
    let args = server_args(exe, config_path);
    let mut arguments = String::new();
    for arg in &args {
        arguments.push_str(&format!("    <string>{}</string>\n", escape(arg)));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
{arguments}  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
  <key>ProcessType</key>
  <string>Interactive</string>
</dict>
</plist>
"#,
        log = escape(&log.display().to_string())
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `~/Library/Logs/rustiio.log`
fn log_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/Logs/rustiio.log")
}

fn uid() -> String {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_else(|| "501".to_string())
}

/// Pokreni `launchctl` bez da njegove poruke završe korisniku pred očima.
fn launchctl(args: &[&str]) -> bool {
    Command::new("launchctl").args(args).output().map(|out| out.status.success()).unwrap_or(false)
}

/// Skini agenta: prvo po oznaci (novi nacin), pa po datoteci, pa stari `unload`.
fn unload(file: &std::path::Path) -> bool {
    let label = format!("gui/{}/{LABEL}", uid());
    let path = file.display().to_string();
    launchctl(&["bootout", &label])
        || launchctl(&["bootout", &format!("gui/{}", uid()), &path])
        || launchctl(&["unload", "-w", &path])
}

/// Je li agent upisan u launchd.
fn is_loaded() -> bool {
    Command::new("launchctl")
        .args(["print", &format!("gui/{}/{LABEL}", uid())])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

pub fn run(action: Action, exe: &Path, config_path: &Path) -> anyhow::Result<()> {
    let file = plist_path()?;
    match action {
        Action::Install => {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).context("mapa LaunchAgents")?;
            }
            let log = log_path();
            std::fs::write(&file, plist(exe, config_path, &log))
                .with_context(|| format!("pisanje {}", file.display()))?;
            // Novi nacin (bootstrap) pa stariji (load -w) ako prvi ne prođe.
            let target = format!("gui/{}", uid());
            let loaded = launchctl(&["bootstrap", &target, &file.display().to_string()])
                || launchctl(&["load", "-w", &file.display().to_string()]);
            if !loaded {
                bail!(
                    "launchd nije prihvatio uslugu — probaj rucno: launchctl bootstrap {target} {}",
                    file.display()
                );
            }
            report("launchd", &file, is_loaded(), &format!("dnevnik: {}", log.display()));
        }
        Action::Uninstall => {
            unload(&file);
            if file.exists() {
                std::fs::remove_file(&file).with_context(|| format!("brisanje {}", file.display()))?;
            }
            report("launchd", &file, false, "usluga je zaustavljena i obrisana");
        }
        Action::Status => {
            report("launchd", &file, is_loaded(), &format!("dnevnik: {}", log_path().display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_carries_label_and_program_arguments() {
        let text = plist(
            Path::new("/Applications/Rustiio.app/Contents/MacOS/rustiio"),
            Path::new("/Users/vaha/Library/Application Support/Rustiio/config.toml"),
            Path::new("/Users/vaha/Library/Logs/rustiio.log"),
        );
        assert!(text.contains("<string>net.vaha.rustiio</string>"));
        assert!(text.contains("<string>run</string>"));
        assert!(text.contains("<string>--config</string>"));
        assert!(text.contains("/Users/vaha/Library/Application Support/Rustiio/config.toml"));
        assert!(text.contains("<key>RunAtLoad</key>"));
        assert!(text.contains("<key>KeepAlive</key>"));
        assert!(text.contains("<string>/Users/vaha/Library/Logs/rustiio.log</string>"));
    }

    #[test]
    fn plist_escapes_xml() {
        let text = plist(Path::new("/tmp/a&b"), Path::new("/tmp/c.toml"), Path::new("/tmp/log"));
        assert!(text.contains("/tmp/a&amp;b"), "znak & mora biti escapiran: {text}");
    }
}
