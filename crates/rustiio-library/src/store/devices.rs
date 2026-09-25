//! Zapamćeni uređaji: stranica Uređaji ne smije biti prazna nakon restarta.
//!
//! Živi registar je u memoriji (`rustiio_profiles::capture::Capture`), pa se ovdje
//! zapisuje svaki put kad se uređaj javi i čita pri dizanju servera.

use rusqlite::params;

use super::Store;

/// Uređaj kako stoji u bazi (bez tipova iz `rustiio-profiles`).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredDevice {
    pub key: String,
    pub ip: String,
    pub user_agent: String,
    pub friendly_name: Option<String>,
    pub profile_id: String,
    /// Profil koji je korisnik izabrao (prazno = automatsko prepoznavanje).
    pub profile_choice: String,
    pub first_seen: u64,
    pub last_seen: u64,
    pub requests: u64,
    pub streams: u64,
    /// DLNA zaglavlja kao JSON objekt.
    pub headers: String,
}

/// Upiši ili osvježi uređaj.
pub fn save(store: &Store, device: &StoredDevice) -> rusqlite::Result<()> {
    let conn = store.conn();
    conn.execute(
        "INSERT INTO devices
             (key, ip, user_agent, friendly_name, profile_id, first_seen, last_seen, requests, streams, headers)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(key) DO UPDATE SET
             ip = excluded.ip,
             user_agent = excluded.user_agent,
             friendly_name = excluded.friendly_name,
             profile_id = excluded.profile_id,
             first_seen = MIN(devices.first_seen, excluded.first_seen),
             last_seen = excluded.last_seen,
             requests = excluded.requests,
             streams = excluded.streams,
             headers = excluded.headers",
        params![
            device.key,
            device.ip,
            device.user_agent,
            // Kolona je NOT NULL: prazno ime je prazan string, ne NULL.
            device.friendly_name.clone().unwrap_or_default(),
            device.profile_id,
            device.first_seen as i64,
            device.last_seen as i64,
            device.requests as i64,
            device.streams as i64,
            device.headers,
        ],
    )?;
    Ok(())
}

/// Izričit izbor profila za uređaj. Prazno polje briše izbor (vraća se na auto).
pub fn set_profile_choice(store: &Store, key: &str, profile_id: &str) -> rusqlite::Result<()> {
    let conn = store.conn();
    // UPSERT: korisnik može odabrati profil i prije nego se uređaj prvi put javio.
    conn.execute(
        "INSERT INTO devices (key, profile_choice) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET profile_choice = excluded.profile_choice",
        params![key, profile_id.trim()],
    )?;
    Ok(())
}

/// Očisti izbor profila na svim uređajima koji su ga koristili.
///
/// Nakon brisanja profila uređaj ne smije ostati vezan na nešto što ne postoji —
/// vraća se na automatsko prepoznavanje. Vraća broj očišćenih uređaja.
pub fn clear_profile_choice(store: &Store, profile_id: &str) -> rusqlite::Result<usize> {
    let conn = store.conn();
    conn.execute(
        "UPDATE devices SET profile_choice = '' WHERE profile_choice = ?1",
        params![profile_id.trim()],
    )
}

/// Obriši uređaj iz pamćenja.
///
/// Briše i njegov izbor profila (isti redak). Ako se uređaj ponovno javi, upisat
/// će se iznova — ovo je popis, ne trajna zabrana.
pub fn delete(store: &Store, key: &str) -> rusqlite::Result<usize> {
    let conn = store.conn();
    conn.execute("DELETE FROM devices WHERE key = ?1", params![key])
}

/// Izabrani profil za uređaj (None = automatski).
pub fn profile_choice(store: &Store, key: &str) -> rusqlite::Result<Option<String>> {
    let conn = store.conn();
    let choice: Option<String> = conn
        .query_row("SELECT profile_choice FROM devices WHERE key = ?1", params![key], |row| row.get(0))
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(choice.filter(|value| !value.trim().is_empty()))
}

/// Svi zapamćeni uređaji, najsvježiji prvi.
pub fn all(store: &Store) -> rusqlite::Result<Vec<StoredDevice>> {
    let conn = store.conn();
    let mut statement = conn.prepare(
        "SELECT key, ip, user_agent, friendly_name, profile_id, first_seen, last_seen,
                requests, streams, headers, profile_choice
         FROM devices ORDER BY last_seen DESC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(StoredDevice {
            key: row.get(0)?,
            ip: row.get(1)?,
            user_agent: row.get(2)?,
            friendly_name: {
                let ime: String = row.get(3)?;
                (!ime.is_empty()).then_some(ime)
            },
            profile_id: row.get(4)?,
            first_seen: row.get::<_, i64>(5)?.max(0) as u64,
            last_seen: row.get::<_, i64>(6)?.max(0) as u64,
            requests: row.get::<_, i64>(7)?.max(0) as u64,
            streams: row.get::<_, i64>(8)?.max(0) as u64,
            headers: row.get(9)?,
            profile_choice: row.get(10)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn device_roundtrip_updates_counters() {
        let store = Store::open_memory().expect("baza");
        let uredjaj = StoredDevice {
            key: "ua:VLC/3.0.23".into(),
            ip: "192.168.1.157".into(),
            user_agent: "VLC/3.0.23 LibVLC/3.0.23".into(),
            friendly_name: None,
            profile_id: "vlc".into(),
            profile_choice: String::new(),
            first_seen: 100,
            last_seen: 200,
            requests: 1,
            streams: 0,
            headers: "{\"transferMode.dlna.org\":\"Streaming\"}".into(),
        };
        save(&store, &uredjaj).expect("upis");

        let drugi = StoredDevice { last_seen: 300, requests: 2, streams: 1, ..uredjaj.clone() };
        save(&store, &drugi).expect("osvjezavanje");

        let svi = all(&store).expect("citanje");
        assert_eq!(svi.len(), 1);
        assert_eq!(svi[0].requests, 2);
        assert_eq!(svi[0].streams, 1);
        assert_eq!(svi[0].first_seen, 100, "prvi susret se ne pomiče");
        assert_eq!(svi[0].last_seen, 300);
        assert!(svi[0].headers.contains("Streaming"));
        assert_eq!(svi[0].friendly_name, None, "prazno ime ostaje None, ne prazan string");
    }

    #[test]
    fn izbor_profila_prezivi_osvjezavanje_uredjaja() {
        let store = Store::open_memory().expect("baza");
        let kljuc = "ua:DLNADOC/1.50 SEC_HHP_[TV] Samsung 6 Series";

        // Korisnik bira profil i prije nego se uredjaj prvi put javio.
        set_profile_choice(&store, kljuc, "samsung-tv").expect("izbor");
        assert_eq!(profile_choice(&store, kljuc).expect("citanje").as_deref(), Some("samsung-tv"));

        // Uredjaj se javlja i red se osvjezava — izbor korisnika mora ostati.
        save(
            &store,
            &StoredDevice {
                key: kljuc.into(),
                ip: "192.168.1.100".into(),
                user_agent: "DLNADOC/1.50".into(),
                friendly_name: Some("TV".into()),
                profile_id: "samsung-tv".into(),
                profile_choice: String::new(),
                first_seen: 1,
                last_seen: 2,
                requests: 3,
                streams: 1,
                headers: "{}".into(),
            },
        )
        .expect("upis");
        assert_eq!(
            profile_choice(&store, kljuc).expect("citanje").as_deref(),
            Some("samsung-tv"),
            "osvjezavanje uredjaja ne smije pregaziti izbor korisnika"
        );

        // Prazno polje vraca automatsko prepoznavanje.
        set_profile_choice(&store, kljuc, "").expect("brisanje");
        assert_eq!(profile_choice(&store, kljuc).expect("citanje"), None);
    }

    #[test]
    fn brisanje_profila_brise_i_izbore_na_uredjajima() {
        let store = Store::open_memory().expect("baza");
        set_profile_choice(&store, "ua:Samsung", "moj-profil").expect("prvi");
        set_profile_choice(&store, "ua:VLC", "moj-profil").expect("drugi");
        set_profile_choice(&store, "ua:Kodi", "vlc").expect("treci");

        let ocisceno = clear_profile_choice(&store, "moj-profil").expect("ciscenje");
        assert_eq!(ocisceno, 2, "obrisana su dva uredjaja koja su koristila profil");
        assert_eq!(profile_choice(&store, "ua:Samsung").expect("citanje"), None);
        assert_eq!(profile_choice(&store, "ua:VLC").expect("citanje"), None);
        assert_eq!(
            profile_choice(&store, "ua:Kodi").expect("citanje").as_deref(),
            Some("vlc"),
            "tudji izbor se ne dira"
        );
    }
}
