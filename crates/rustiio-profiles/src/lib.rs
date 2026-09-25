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

#[cfg(test)]
mod proba_ciljnih_kodeka {
    use super::builtin;

    /// Serviio svoj Samsung profil rjesava s `targetACodec="ac3"` — AAC u MPEG-TS-u
    /// Samsung ne pusti (vrti krug bez slike), pa TV profili moraju ciljati AC-3.
    #[test]
    fn ispisi_i_provjeri_ciljne_kodeke() {
        let set = builtin::load();
        for id in ["generic", "samsung-tv", "samsung-old"] {
            let profil = set.get(id).unwrap_or_else(|| panic!("nema {id}"));
            println!(
                "{id}: video={} audio={} kanala={} bitrate={:?} kontejner={}",
                profil.transcode.video_codec,
                profil.transcode.audio_codec,
                profil.transcode.audio_channels,
                profil.transcode.max_bitrate_kbps,
                profil.transcode.container
            );
        }
        for id in ["generic", "samsung-tv", "samsung-old"] {
            let profil = set.get(id).expect(id);
            assert_eq!(profil.transcode.audio_codec, "ac3", "{id} mora ciljati AC-3");
        }
    }
}
