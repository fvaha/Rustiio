//! Capture: biljezi sto je koji uredjaj STVARNO trazio.
//!
//! Ovo je odgovor na "kako napisati profil za TV koji ne poznajemo": ne pogadjamo,
//! nego zapisemo User-Agent, DLNA headere i odabrani profil, pa iz toga generiramo
//! TOML profil koji korisnik pregleda i spremi. Zapisi se drze u memoriji (Faza 4 ih
//! prikazuje u UI-ju, Faza 3 seli u SQLite).

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::matcher::DeviceIdentity;
use crate::profile::Profile;

/// Sto znamo o jednom uredjaju koji je nesto zatrazio.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub key: String,
    pub ip: String,
    pub user_agent: String,
    pub friendly_name: Option<String>,
    pub profile_id: String,
    pub first_seen: u64,
    pub last_seen: u64,
    pub requests: u64,
    /// Zadnje vidjeni DLNA/HTTP headeri (npr. `transferMode.dlna.org`, `getcontentFeatures`).
    pub headers: BTreeMap<String, String>,
    /// Objekti koje je uredjaj pustao (za "sto ovaj TV voli gledati").
    pub streams: Vec<String>,
}

/// Registar zapisa (jedan po serveru).
#[derive(Default)]
pub struct Capture {
    devices: Mutex<HashMap<String, DeviceRecord>>,
}

impl Capture {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ubaci zapise iz baze pri dizanju (živi zapis ima prednost).
    pub fn seed(&self, records: Vec<DeviceRecord>) {
        let Ok(mut devices) = self.devices.lock() else {
            return;
        };
        for record in records {
            devices.entry(record.key.clone()).or_insert(record);
        }
    }

    /// Zabiljezi zahtjev; `headers` su samo DLNA-relevantna zaglavlja.
    pub fn record(
        &self,
        identity: &DeviceIdentity,
        profile_id: &str,
        headers: &[(String, String)],
    ) -> DeviceRecord {
        let key = identity.key();
        let now = now_secs();
        let Ok(mut devices) = self.devices.lock() else {
            return DeviceRecord::default();
        };

        let record = devices.entry(key.clone()).or_insert_with(|| DeviceRecord {
            key: key.clone(),
            first_seen: now,
            ..Default::default()
        });

        record.last_seen = now;
        record.requests += 1;
        record.ip = identity.ip.clone().unwrap_or_default();
        if let Some(agent) = identity.user_agent.clone() {
            record.user_agent = agent;
        }
        if let Some(name) = identity.friendly_name.clone() {
            record.friendly_name = Some(name);
        }
        record.profile_id = profile_id.to_string();
        for (name, value) in headers {
            record.headers.insert(name.clone(), value.clone());
        }
        record.clone()
    }

    /// Zapisi da je uredjaj pustio konkretan objekt.
    pub fn note_stream(&self, identity: &DeviceIdentity, object_id: &str) {
        let key = identity.key();
        let Ok(mut devices) = self.devices.lock() else { return };
        if let Some(record) = devices.get_mut(&key) {
            if !record.streams.iter().any(|seen| seen == object_id) {
                record.streams.push(object_id.to_string());
                if record.streams.len() > 50 {
                    record.streams.remove(0);
                }
            }
        }
    }

    /// Svi zapisi, najsvjeziji prvi.
    pub fn all(&self) -> Vec<DeviceRecord> {
        let Ok(devices) = self.devices.lock() else { return Vec::new() };
        let mut out: Vec<DeviceRecord> = devices.values().cloned().collect();
        out.sort_by_key(|a| std::cmp::Reverse(a.last_seen));
        out
    }

    pub fn get(&self, key: &str) -> Option<DeviceRecord> {
        self.devices.lock().ok()?.get(key).cloned()
    }

    /// Zaboravi uređaj u živom registru (korisnik ga je obrisao u sučelju).
    pub fn forget(&self, key: &str) -> bool {
        match self.devices.lock() {
            Ok(mut uredjaji) => uredjaji.remove(key).is_some(),
            // Otrovana brava: popis iz baze je ionako izvor istine.
            Err(_) => false,
        }
    }

    pub fn len(&self) -> usize {
        self.devices.lock().map(|devices| devices.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Napravi TOML profil iz zapisa: polazi od profila koji je uredjaj dobio
    /// (ili generic), a pravilo matchanja postaje njegov stvarni User-Agent.
    pub fn profile_toml(&self, key: &str, base: &Profile, id: &str) -> Option<String> {
        let record = self.get(key)?;
        let mut profile = base.clone();
        profile.id = id.to_string();
        profile.name =
            format!("{} (iz capture-a)", record.friendly_name.clone().unwrap_or_else(|| record.ip.clone()));
        profile.description = format!(
            "Generirano iz prometa uredjaja {} ({}, {} zahtjeva). Provjeri capabilities prije spremanja.",
            record.user_agent, record.ip, record.requests
        );
        profile.rules.user_agent = vec![record.user_agent.clone()];
        if let Some(name) = &record.friendly_name {
            profile.rules.friendly_name = vec![name.clone()];
        }
        profile.rules.ip = Vec::new();
        toml::to_string_pretty(&profile).ok()
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> DeviceIdentity {
        DeviceIdentity {
            user_agent: Some("SEC_HHP_[TV]ExampleTV/1.0".to_string()),
            friendly_name: Some("[TV] Samsung 6 Series".to_string()),
            ip: Some("10.0.0.100".to_string()),
            device_type: None,
        }
    }

    #[test]
    fn records_requests_and_headers() {
        let capture = Capture::new();
        let headers = vec![
            ("transfermode.dlna.org".to_string(), "Streaming".to_string()),
            ("user-agent".to_string(), "SEC_HHP_[TV]ExampleTV/1.0".to_string()),
        ];
        capture.record(&identity(), "samsung-tv", &headers);
        let second = capture.record(&identity(), "samsung-tv", &headers);

        assert_eq!(capture.len(), 1, "isti uredjaj se ne duplicira");
        assert_eq!(second.requests, 2);
        assert_eq!(second.profile_id, "samsung-tv");
        assert_eq!(second.headers.get("transfermode.dlna.org").unwrap(), "Streaming");
        assert!(second.last_seen >= second.first_seen);
    }

    #[test]
    fn streams_are_kept_unique_and_capped() {
        let capture = Capture::new();
        capture.record(&identity(), "samsung-tv", &[]);
        for index in 0..60 {
            capture.note_stream(&identity(), &format!("res/{index}"));
        }
        capture.note_stream(&identity(), "res/59");
        let record = capture.get(&identity().key()).unwrap();
        assert_eq!(record.streams.len(), 50, "lista je ogranicena");
        assert_eq!(record.streams.last().unwrap(), "res/59");
    }

    #[test]
    fn generated_profile_matches_that_device_and_is_valid_toml() {
        let capture = Capture::new();
        capture.record(&identity(), "samsung-tv", &[]);
        let base = crate::builtin::load().get("samsung-tv").unwrap().clone();

        let text = capture.profile_toml(&identity().key(), &base, "moj-samsung").expect("toml");
        let parsed: Profile = toml::from_str(&text).expect("profil se moze parsirati");
        assert_eq!(parsed.id, "moj-samsung");
        assert_eq!(parsed.rules.user_agent, vec!["SEC_HHP_[TV]ExampleTV/1.0"]);
        assert!(parsed.rules.ip.is_empty(), "IP se ne upisuje u pravila");
        assert!(parsed.supports_video_codec("hevc"), "capabilities se naslijede");

        // i taj novi profil mora prepoznati isti uredjaj
        let mut set = crate::builtin::load();
        set.upsert(parsed);
        assert_eq!(set.identify(&identity()).profile.id, "moj-samsung");
    }

    #[test]
    fn unknown_key_gives_none() {
        let capture = Capture::new();
        assert!(capture.profile_toml("ua:nema", &Profile::default(), "x").is_none());
    }
}
