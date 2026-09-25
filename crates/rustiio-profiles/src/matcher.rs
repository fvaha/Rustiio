//! Matcher: od zaglavlja zahtjeva do konkretnog profila.

use std::path::Path;

use tracing::{debug, warn};

use crate::profile::Profile;

/// Ono sto o uredjaju znamo iz samog zahtjeva.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub user_agent: Option<String>,
    pub friendly_name: Option<String>,
    pub ip: Option<String>,
    pub device_type: Option<String>,
}

impl DeviceIdentity {
    /// Stabilan kljuc uredjaja za capture i watch-state.
    ///
    /// Prednost ima **adresa**: DLNA uredjaj salje User-Agent samo na nekim
    /// zahtjevima (Samsung ga ne posalje kad trazi stream), pa bi isti televizor
    /// inace zavrsio kao dva uredjaja — jedan prepoznat po UA, drugi bez njega.
    /// User-Agent i dalje odlucuje koji profil vrijedi (matcher gleda cijeli
    /// identitet), ali zapis i vezani profil ostaju jedan po uredjaju.
    pub fn key(&self) -> String {
        // Lokalne adrese (127.x) ne razlikuju uredjaje, pa tamo UA ostaje kljuc.
        let adresa = self
            .ip
            .as_deref()
            .map(str::trim)
            .filter(|ip| !ip.is_empty() && !ip.starts_with("127.") && ip.trim() != "::1")
            .map(|ip| {
                ip.split_once(':')
                    .map(|(cisto, _)| cisto.to_string())
                    .unwrap_or_else(|| ip.to_string())
            });
        if let Some(ip) = adresa {
            return format!("ip:{ip}");
        }
        if let Some(agent) = self.user_agent.as_deref().filter(|value| !value.is_empty()) {
            return format!("ua:{}", agent.trim());
        }
        if let Some(name) = self.friendly_name.as_deref().filter(|value| !value.is_empty()) {
            return format!("name:{}", name.trim());
        }
        format!("ip:{}", self.ip.as_deref().unwrap_or("nepoznat"))
    }
}

#[derive(Debug, Clone)]
pub struct MatchOutcome<'a> {
    pub profile: &'a Profile,
    pub score: u32,
    /// Zasto je odabran — ide u log i u UI ("prepoznat po ...").
    pub reasons: Vec<String>,
}

/// Skup profila (ugradjeni + korisnicki) s matcherom.
#[derive(Debug, Clone, Default)]
pub struct ProfileSet {
    profiles: Vec<Profile>,
    generic: Profile,
}

impl ProfileSet {
    pub fn new(profiles: Vec<Profile>) -> Self {
        let generic = profiles
            .iter()
            .find(|profile| profile.rules.is_empty() && profile.id != "generic")
            .cloned()
            .unwrap_or_default();
        Self { profiles, generic }
    }

    pub fn all(&self) -> &[Profile] {
        &self.profiles
    }

    pub fn ids(&self) -> Vec<String> {
        self.profiles.iter().map(|profile| profile.id.clone()).collect()
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    /// Profil koji se koristi kad nista ne upali.
    pub fn generic(&self) -> &Profile {
        &self.generic
    }

    /// Nadji najbolji profil; nikad ne pada — u najgorem slucaju vrati generic.
    pub fn identify(&self, identity: &DeviceIdentity) -> MatchOutcome<'_> {
        let mut best: Option<MatchOutcome<'_>> = None;

        for profile in &self.profiles {
            let (score, reasons) = score_profile(profile, identity);
            if score == 0 {
                continue;
            }
            let better = match &best {
                Some(current) => score > current.score,
                None => true,
            };
            if better {
                best = Some(MatchOutcome { profile, score, reasons });
            }
        }

        match best {
            Some(outcome) => {
                debug!(
                    profile = %outcome.profile.id,
                    score = outcome.score,
                    reasons = %outcome.reasons.join("; "),
                    "profil odabran"
                );
                outcome
            }
            None => {
                debug!(device = %identity.key(), "nijedan profil ne odgovara — koristim generic");
                MatchOutcome { profile: &self.generic, score: 0, reasons: vec!["generic".to_string()] }
            }
        }
    }

    /// Dodaj/pregazi profil (korisnicki profili imaju prednost nad ugradjenima).
    pub fn upsert(&mut self, profile: Profile) {
        match self.profiles.iter().position(|existing| existing.id == profile.id) {
            Some(index) => self.profiles[index] = profile,
            None => self.profiles.push(profile),
        }
    }

    /// Ucitaj sve `*.toml` iz mape; greske se ne bacaju nego vrate (jedan los
    /// profil ne smije srusiti server).
    pub fn load_dir(&mut self, dir: &Path) -> Vec<String> {
        let mut problems = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) => {
                if err.kind() != std::io::ErrorKind::NotFound {
                    warn!(dir = %dir.display(), error = %err, "ne mogu citati mapu profila");
                }
                return problems;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|ext| ext.eq_ignore_ascii_case("toml")).unwrap_or(false) {
                match std::fs::read_to_string(&path)
                    .map_err(|err| err.to_string())
                    .and_then(|text| toml::from_str::<Profile>(&text).map_err(|err| err.to_string()))
                {
                    Ok(profile) => {
                        debug!(id = %profile.id, file = %path.display(), "korisnicki profil ucitan");
                        self.upsert(profile);
                    }
                    Err(err) => problems.push(format!("{}: {err}", path.display())),
                }
            }
        }
        problems
    }
}

fn score_profile(profile: &Profile, identity: &DeviceIdentity) -> (u32, Vec<String>) {
    let mut score = 0;
    let mut reasons = Vec::new();

    if let Some(ip) = identity.ip.as_deref() {
        if profile.rules.ip.iter().any(|rule| rule.trim() == ip.trim()) {
            score += 100;
            reasons.push(format!("IP {ip}"));
        }
    }

    let (agent_score, agent_reasons) = best_rule_score(
        identity.user_agent.as_deref(),
        &profile.rules.user_agent,
        "User-Agent sadrzi",
        20,
        25,
    );
    score += agent_score;
    reasons.extend(agent_reasons);

    let (name_score, name_reasons) = best_rule_score(
        identity.friendly_name.as_deref(),
        &profile.rules.friendly_name,
        "ime sadrzi",
        10,
        15,
    );
    score += name_score;
    reasons.extend(name_reasons);

    let (type_score, type_reasons) =
        best_rule_score(identity.device_type.as_deref(), &profile.rules.device_type, "tip", 8, 10);
    score += type_score;
    reasons.extend(type_reasons);

    (score, reasons)
}

/// Bodovi za jedno polje: **najjace pravilo nosi bodove**, broj pogodaka samo malo
/// pomaze. Tako profil s punim `User-Agent`-om pobijedi opceniti (`SEC_HHP`), a
/// profil koji pogodi tri slaba pravila ne pobijedi jak pogodak.
fn best_rule_score(
    haystack: Option<&str>,
    rules: &[String],
    label: &str,
    base: u32,
    max_bonus: u32,
) -> (u32, Vec<String>) {
    let Some(haystack) = haystack else { return (0, Vec::new()) };
    let matched: Vec<&String> = rules.iter().filter(|rule| contains_ignore_case(haystack, rule)).collect();
    if matched.is_empty() {
        return (0, Vec::new());
    }

    let best = matched.iter().map(|rule| base + specificity(rule, max_bonus)).max().unwrap_or(base);
    let count_bonus = (matched.len() as u32).min(3);
    let reasons = matched.iter().map(|rule| format!("{label} \"{rule}\"")).collect();
    (best + count_bonus, reasons)
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    let needle = needle.trim();
    !needle.is_empty() && haystack.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}

/// Koliko dodatnih bodova nosi duljina pravila (svaka 4 znaka = 1 bod, do `max`).
fn specificity(rule: &str, max: u32) -> u32 {
    (rule.trim().chars().count() as u32 / 4).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin;

    fn samsung() -> DeviceIdentity {
        DeviceIdentity {
            user_agent: Some("SEC_HHP_[TV]UE55MU6172/1.0".to_string()),
            friendly_name: Some("[TV] Samsung 6 Series (55)".to_string()),
            ip: Some("192.168.1.100".to_string()),
            device_type: None,
        }
    }

    #[test]
    fn identifies_samsung_tv() {
        let set = builtin::load();
        let outcome = set.identify(&samsung());
        assert_eq!(outcome.profile.id, "samsung-tv");
        assert!(outcome.score >= 20);
        assert!(outcome.reasons.iter().any(|reason| reason.contains("User-Agent")));
    }

    #[test]
    fn explicit_ip_rule_beats_user_agent() {
        let mut set = builtin::load();
        let mut forced = Profile {
            id: "sharp-dnevni".to_string(),
            name: "Sharp (rucno)".to_string(),
            ..Profile::default()
        };
        forced.rules.ip = vec!["192.168.1.55".to_string()];
        set.upsert(forced);

        let identity = DeviceIdentity {
            user_agent: Some("SEC_HHP_[TV]UE55MU6172/1.0".to_string()),
            friendly_name: None,
            ip: Some("192.168.1.55".to_string()),
            device_type: None,
        };
        assert_eq!(set.identify(&identity).profile.id, "sharp-dnevni");
    }

    #[test]
    fn unknown_device_gets_generic() {
        let set = builtin::load();
        let identity =
            DeviceIdentity { user_agent: Some("NekiNepoznatiKlijent/9.9".to_string()), ..Default::default() };
        let outcome = set.identify(&identity);
        assert_eq!(outcome.profile.id, "generic");
        assert_eq!(outcome.score, 0);
    }

    #[test]
    fn vlc_and_kodi_are_known_clients() {
        let set = builtin::load();
        let vlc =
            DeviceIdentity { user_agent: Some("VLC/3.0.21 LibVLC/3.0.21".into()), ..Default::default() };
        assert_eq!(set.identify(&vlc).profile.id, "vlc");

        let kodi =
            DeviceIdentity { user_agent: Some("Kodi/21.0 (X11; Linux x86_64)".into()), ..Default::default() };
        assert_eq!(set.identify(&kodi).profile.id, "kodi");
    }

    #[test]
    fn more_specific_rule_wins_over_general_one() {
        let set = crate::builtin::load();
        let mut captured = Profile { id: "moj-samsung".to_string(), ..Profile::default() };
        // Tako izgleda profil koji napravi capture: pun UA + puno ime uredjaja.
        captured.rules.user_agent = vec!["SEC_HHP_[TV]UE55MU6172/1.0".to_string()];
        captured.rules.friendly_name = vec!["[TV] Samsung 6 Series (55)".to_string()];

        let general = set.identify(&samsung()).score;
        let mut with_capture = set.clone();
        with_capture.upsert(captured);
        let outcome = with_capture.identify(&samsung());

        assert_eq!(outcome.profile.id, "moj-samsung", "puni User-Agent je specificniji od \"SEC_HHP\"");
        assert!(outcome.score > general);
    }

    #[test]
    fn device_key_is_the_address_so_one_tv_is_one_device() {
        // Samsung posalje User-Agent samo na nekim zahtjevima; da kljuc ostane UA,
        // isti televizor bi zavrsio kao dva uredjaja (jedan s profilom, drugi bez).
        let mut tv = samsung();
        tv.ip = Some("192.168.1.100".into());
        assert_eq!(tv.key(), "ip:192.168.1.100");
        // Lokalne adrese ne razlikuju uredjaje — tamo UA ostaje kljuc.
        tv.ip = Some("127.0.0.1".into());
        assert_eq!(tv.key(), "ua:SEC_HHP_[TV]UE55MU6172/1.0");
        let bare = DeviceIdentity { ip: Some("10.0.0.5".into()), ..Default::default() };
        assert_eq!(bare.key(), "ip:10.0.0.5");
    }

    #[test]
    fn load_dir_merges_user_profiles_and_reports_broken_files() {
        let dir = std::env::temp_dir().join(format!("rustiio-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("moj.toml"),
            "id = \"moj-tv\"\nname = \"Moj TV\"\n[match]\nuser_agent = [\"MojTV\"]\n",
        )
        .unwrap();
        std::fs::write(dir.join("pokvaren.toml"), "id = \"x\"\nname = 5\n").unwrap();

        let mut set = builtin::load();
        let problems = set.load_dir(&dir);
        assert_eq!(problems.len(), 1, "pokvaren fajl se prijavi, ne baci: {problems:?}");
        assert!(set.get("moj-tv").is_some());

        let identity = DeviceIdentity { user_agent: Some("MojTV/Smart 1".into()), ..Default::default() };
        assert_eq!(set.identify(&identity).profile.id, "moj-tv");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_replaces_same_id() {
        let mut set = builtin::load();
        let before = set.get("samsung-tv").unwrap().name.clone();
        let replacement =
            Profile { id: "samsung-tv".to_string(), name: "Samsung (moj)".to_string(), ..Profile::default() };
        set.upsert(replacement);
        assert_ne!(set.get("samsung-tv").unwrap().name, before);
        assert_eq!(set.all().iter().filter(|p| p.id == "samsung-tv").count(), 1);
    }
}
