//! Identitet uredaja: UDN se generira jednom i cuva u configu (da TV ne dobije
//! novi uredaj nakon restarta), friendly name se izvede iz hostnamea.

use std::path::Path;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct DeviceIdentity {
    /// `uuid:xxxxxxxx-...` (s prefiksom, kako ide u device.xml i SSDP).
    pub udn: String,
    pub friendly_name: String,
    pub model_name: String,
    pub model_number: String,
    pub manufacturer: String,
    pub serial_number: String,
}

impl DeviceIdentity {
    /// Osigura UDN i friendly name u configu (upise ih ako fale) i vrati identitet.
    pub fn ensure(cfg: &mut Config, config_path: &Path) -> anyhow::Result<Self> {
        let mut dirty = false;

        let udn = match cfg.server.udn.clone() {
            Some(u) => normalize_udn(&u),
            None => {
                let u = format!("uuid:{}", uuid::Uuid::new_v4());
                cfg.server.udn = Some(u.clone());
                dirty = true;
                u
            }
        };

        let friendly_name = match cfg.server.friendly_name.clone() {
            Some(n) if !n.trim().is_empty() => n,
            _ => {
                let host = hostname::get()
                    .ok()
                    .and_then(|h| h.into_string().ok())
                    .unwrap_or_else(|| "local".to_string());
                let name = format!("Rustiio ({host})");
                cfg.server.friendly_name = Some(name.clone());
                dirty = true;
                name
            }
        };

        if dirty && !config_path.as_os_str().is_empty() {
            cfg.save(config_path)?;
        }

        Ok(Self {
            serial_number: udn.trim_start_matches("uuid:").to_string(),
            udn,
            friendly_name,
            model_name: "Rustiio Media Server".to_string(),
            model_number: crate::VERSION.to_string(),
            manufacturer: "Rustiio".to_string(),
        })
    }

    /// Dio bez `uuid:` prefiksa.
    pub fn uuid(&self) -> &str {
        self.udn.strip_prefix("uuid:").unwrap_or(&self.udn)
    }

    /// USN za zadani target (`uuid:X` ostaje sam, ostalo ide `uuid:X::NT`).
    pub fn usn(&self, nt: &str) -> String {
        if nt.eq_ignore_ascii_case(&self.udn) || nt.eq_ignore_ascii_case(self.uuid()) {
            self.udn.clone()
        } else {
            format!("{}::{}", self.udn, nt)
        }
    }
}

fn normalize_udn(raw: &str) -> String {
    let raw = raw.trim();
    match raw.strip_prefix("uuid:") {
        Some(rest) => format!("uuid:{}", rest.trim().to_lowercase()),
        None => format!("uuid:{}", raw.to_lowercase()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_creates_and_persists_udn() {
        let dir = std::env::temp_dir().join(format!("rustiio-id-{}", uuid::Uuid::new_v4()));
        let path = dir.join("config.toml");
        let mut cfg = Config::default();

        let id1 = DeviceIdentity::ensure(&mut cfg, &path).expect("ensure");
        assert!(id1.udn.starts_with("uuid:"));
        assert!(path.exists());

        let mut cfg2 = Config::load(&path).expect("load");
        let id2 = DeviceIdentity::ensure(&mut cfg2, &path).expect("ensure again");
        assert_eq!(id1.udn, id2.udn, "UDN mora prezivjeti restart");
        assert_eq!(id1.friendly_name, id2.friendly_name);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn usn_for_rootdevice_and_self() {
        let mut cfg = Config::default();
        cfg.server.udn = Some("uuid:abc-123".to_string());
        cfg.server.friendly_name = Some("Rustiio".to_string());
        let id = DeviceIdentity::ensure(&mut cfg, Path::new("")).expect("ensure");
        assert_eq!(id.usn(&id.udn), "uuid:abc-123");
        assert_eq!(id.usn("upnp:rootdevice"), "uuid:abc-123::upnp:rootdevice");
    }

    #[test]
    fn udn_without_prefix_is_normalized() {
        let mut cfg = Config::default();
        cfg.server.udn = Some("ABC-123".to_string());
        let id = DeviceIdentity::ensure(&mut cfg, Path::new("")).expect("ensure");
        assert_eq!(id.udn, "uuid:abc-123");
    }
}
