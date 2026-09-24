//! Profili uredaja — srce univerzalnosti Rustiia.
//!
//! Nijedan TV nije hardkodiran u kodu: sve sto server zna o nekom uredjaju zivi u
//! TOML profilu (ugradjeni + korisnicki). Matcher pogadja profil po `User-Agent`-u,
//! imenu uredjaja, tipu i IP-u; capture biljezi sto je uredjaj stvarno trazio, pa se
//! iz toga jednim pozivom napravi novi profil.
//!
//! Sektori:
//! - [`profile`]  — model profila (capabilities + transcode cilj + DLNA detalji)
//! - [`builtin`]  — ugradjena baza profila (TOML, ugradjen u binarni fajl)
//! - [`matcher`]  — identifikacija uredjaja i odabir profila
//! - [`capture`]  — zapis stvarnog ponasanja uredjaja (+ generator profila)

pub mod builtin;
pub mod capture;
pub mod matcher;
pub mod profile;

pub use capture::{Capture, DeviceRecord};
pub use matcher::{DeviceIdentity, MatchOutcome, ProfileSet};
pub use profile::{
    AudioCaps, DlnaCaps, MatchRules, Profile, SubtitleCaps, SubtitleMode, TranscodeTarget, VideoCaps,
};
