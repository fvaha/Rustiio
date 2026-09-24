//! Ugradjena baza profila (TOML ugradjen u binarni fajl).
//!
//! Namjerno **ne** hardkodiramo uredjaje u kodu: datoteke zive u `profiles/`, a
//! ovdje se samo ucitavaju. Korisnik ih moze pregaziti svojima (isti `id`).

use tracing::warn;

use crate::matcher::ProfileSet;
use crate::profile::Profile;

/// Ime mape u kojoj korisnik moze imati svoje profile (relativno na config mapu).
pub const USER_PROFILES_DIR: &str = "profiles";

/// Svi ugradjeni profili. Prvi (`generic`) je fallback i nema pravila matchanja.
const BUILTIN: &[(&str, &str)] = &[
    ("generic", include_str!("../profiles/00-generic.toml")),
    ("samsung-tv", include_str!("../profiles/samsung-tv.toml")),
    ("samsung-old", include_str!("../profiles/samsung-old.toml")),
    ("lg-webos", include_str!("../profiles/lg-webos.toml")),
    ("lg-netcast", include_str!("../profiles/lg-netcast.toml")),
    ("sony-bravia", include_str!("../profiles/sony-bravia.toml")),
    ("panasonic-viera", include_str!("../profiles/panasonic-viera.toml")),
    ("philips-tv", include_str!("../profiles/philips-tv.toml")),
    ("sharp-aquos", include_str!("../profiles/sharp-aquos.toml")),
    ("android-tv", include_str!("../profiles/android-tv.toml")),
    ("chromecast", include_str!("../profiles/chromecast.toml")),
    ("xbox", include_str!("../profiles/xbox.toml")),
    ("playstation", include_str!("../profiles/playstation.toml")),
    ("vlc", include_str!("../profiles/vlc.toml")),
    ("kodi", include_str!("../profiles/kodi.toml")),
    ("plex-client", include_str!("../profiles/plex-client.toml")),
    ("web-browser", include_str!("../profiles/web-browser.toml")),
];

/// Ucitaj ugradjene profile. Pokvaren ugradjeni profil se preskace uz `warn!`
/// (nikad ne rusi start servera).
pub fn load() -> ProfileSet {
    let mut profiles = Vec::with_capacity(BUILTIN.len());
    for (expected_id, text) in BUILTIN {
        match toml::from_str::<Profile>(text) {
            Ok(profile) => profiles.push(profile),
            Err(err) => warn!(id = expected_id, error = %err, "ugradjeni profil se ne moze parsirati"),
        }
    }
    ProfileSet::new(profiles)
}

/// Ugradjeni profili + korisnicki iz `config_dir/profiles/*.toml`.
pub fn load_with_user(profile_dir: Option<&std::path::Path>) -> (ProfileSet, Vec<String>) {
    let mut set = load();
    let problems = match profile_dir {
        Some(dir) => set.load_dir(dir),
        None => Vec::new(),
    };
    (set, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::SubtitleMode;

    #[test]
    fn every_builtin_profile_parses_and_has_unique_id() {
        let set = load();
        assert!(set.all().len() >= 15, "ocekujemo siroku bazu, imamo {}", set.all().len());

        let mut ids: Vec<String> = set.ids();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplikat id-a u ugradjenim profilima");

        for profile in set.all() {
            assert!(!profile.name.is_empty(), "profil {} bez imena", profile.id);
            assert!(
                !profile.video.codecs.is_empty(),
                "profil {} bez video kodeka — bio bi beskorisan",
                profile.id
            );
            assert!(profile.transcode.max_bitrate_kbps > 0, "profil {} bez bitratea", profile.id);
        }
    }

    #[test]
    fn generic_profile_has_no_match_rules_and_is_the_fallback() {
        let set = load();
        let generic = set.get("generic").expect("generic profil");
        assert!(generic.rules.is_empty(), "generic ne smije imati pravila");
        assert_eq!(set.generic().id, "generic");
    }

    #[test]
    fn samsung_2016_plus_knows_hevc_and_burn_in_for_old_ones() {
        let set = load();
        let modern = set.get("samsung-tv").unwrap();
        assert!(modern.supports_video_codec("hevc"), "2016+ Samsung dekodira HEVC");
        assert!(modern.fits_video(3840, 2160));

        let old = set.get("samsung-old").unwrap();
        assert!(!old.supports_video_codec("hevc"), "stariji Samsung ne dekodira HEVC");
        assert_eq!(old.subtitle_mode(), SubtitleMode::Burn, "stariji modeli ne citaju srt");
    }

    #[test]
    fn kodi_and_vlc_can_play_almost_anything() {
        let set = load();
        for id in ["kodi", "vlc"] {
            let profile = set.get(id).unwrap();
            for codec in ["h264", "hevc", "av1", "mpeg4"] {
                assert!(profile.supports_video_codec(codec), "{id} bi trebao podrzavati {codec}");
            }
            assert!(profile.supports_audio_track("dts", 8), "{id} propusta i DTS 7.1");
        }
    }
}
