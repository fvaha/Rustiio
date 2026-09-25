//! Titlovi: jezici, prepoznavanje iz imena datoteka i model jedne staze.
//!
//! Tri izvora koje podrzavamo:
//! * **vanjski** (`Film.srt`, `Film.en.srt`, `Subs/Film.hrv.forced.srt`) — `Source::External`
//! * **ugradjeni** u kontejneru (`mkv` staza, `ffprobe` je vidi) — `Source::Embedded`
//! * **urezani** (burn-in) — nije staza nego nacin prikaza; odlucuje profil uredjaja, a izvor
//!   je jedna od gornje dvije staze.
//!
//! Jezgro je imenovanje: TV-u se u popisu titlova mora vidjeti `English`, `Croatian`,
//! `Spanish` — ne `Language 1` (kako radi Serviio) ni `und`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Odakle titl dolazi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// Datoteka uz video.
    External(PathBuf),
    /// Staza unutar kontejnera (`ffprobe` `index`).
    Embedded { index: u32 },
}

/// Jedna staza titla (vanjska datoteka ili ugradjena u kontejneru).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleTrack {
    pub source: Source,
    /// `srt`, `subrip`, `ass`, `hdmv_pgs_subtitle`…
    pub codec: String,
    /// ISO 639-1/639-2 kod iz metapodataka (`en`, `eng`, `hrv`); `None` kad pise `und`.
    pub language: Option<String>,
    /// Naslov staze iz metapodataka (npr. `SDH`, `Forced`, `Commentary`).
    pub label: Option<String>,
    pub forced: bool,
    pub sdh: bool,
    pub default: bool,
}

impl SubtitleTrack {
    /// Moze li se posluziti kao tekst (SRT)? Slikovni titlovi (PGS, VobSub) ne mogu.
    pub fn is_text(&self) -> bool {
        !matches!(
            self.codec.to_ascii_lowercase().as_str(),
            "hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle" | "xsub" | "vobsub" | "pgs"
        )
    }

    pub fn language_name(&self) -> Option<&'static str> {
        self.language.as_deref().and_then(language_name)
    }

    /// Ime koje korisnik vidi: `English`, `Croatian (forced)`, `English (SDH)`.
    /// Kad jezika nema nigdje, uzima naslov staze, pa `codec` — nikad izmisljeni broj jezika.
    pub fn display_name(&self) -> String {
        let osnova =
            self.language_name()
                .map(str::to_string)
                .or_else(|| self.label.as_deref().map(ocisti_label))
                .unwrap_or_else(|| {
                    if self.codec.is_empty() { "Subtitles".to_string() } else { self.codec.clone() }
                });
        let mut dodatak: Vec<&str> = Vec::new();
        if self.forced {
            dodatak.push("forced");
        }
        if self.sdh {
            dodatak.push("SDH");
        }
        // Ako je naslov staze vec rekao `SDH`/`forced`, ne ponavljaj.
        let osnova_bez = osnova.to_ascii_lowercase();
        let dodatak: Vec<&str> =
            dodatak.into_iter().filter(|oznaka| !osnova_bez.contains(&oznaka.to_ascii_lowercase())).collect();
        if dodatak.is_empty() { osnova } else { format!("{osnova} ({})", dodatak.join(", ")) }
    }

    pub fn serve_name(&self, video_stem: &str) -> String {
        match &self.source {
            Source::External(putanja) => {
                putanja.file_name().map(|ime| ime.to_string_lossy().to_string()).unwrap_or_default()
            }
            // Ugradjeni se vadi iz kontejnera u SRT; ime nosi jezik, ne broj staze.
            Source::Embedded { index } => {
                let oznaka = self
                    .language
                    .clone()
                    .or_else(|| self.language_name().map(|ime| ime.to_ascii_lowercase()))
                    .unwrap_or_else(|| index.to_string());
                let forced = if self.forced { ".forced" } else { "" };
                format!("{video_stem}.{oznaka}{forced}.srt")
            }
        }
    }

    /// Vanjska datoteka ako postoji (za urezivanje i posluzivanje).
    pub fn path(&self) -> Option<&Path> {
        match &self.source {
            Source::External(putanja) => Some(putanja.as_path()),
            Source::Embedded { .. } => None,
        }
    }
}

/// Naslov staze bez smeca (`eng - SDH`, `[Forced]` → `SDH`, `Forced`).
fn ocisti_label(label: &str) -> String {
    label
        .trim_matches(|znak: char| znak == '[' || znak == ']' || znak == '(' || znak == ')')
        .trim()
        .to_string()
}

/// Izvuci oznake (jezik, forced, sdh) iz djela imena nakon imena videa.
/// `"en"` → engleski; `"hrv.forced"` → hrvatski + forced; `"sdh"` → SDH bez jezika.
pub fn parse_tokens(repak: &str) -> (Option<String>, bool, bool) {
    let mut jezik: Option<String> = None;
    let mut forced = false;
    let mut sdh = false;
    for dio in repak.split(['.', '_', '-', ' ']).filter(|dio| !dio.trim().is_empty()) {
        let dio = dio.trim().to_ascii_lowercase();
        match dio.as_str() {
            "forced" | "f" => forced = true,
            "sdh" | "hi" | "cc" => sdh = true,
            // Tehnicki repak (`1080p`, `x265`) nije jezik i ne smije proci kao oznaka.
            _ if je_detalj_kvalitete(&dio) => {}
            _ => {
                if jezik.is_none() {
                    if let Some(kod) = language_code(&dio) {
                        jezik = Some(kod);
                    }
                }
            }
        }
    }
    (jezik, forced, sdh)
}

/// Je li dio repka tehnicki detalj (`1080p`, `x265`, `webrip`) — takav `Film.webrip.srt`
/// ne smije dobiti lazni jezik.
fn je_detalj_kvalitete(dio: &str) -> bool {
    dio.chars().any(|znak| znak.is_ascii_digit())
        || matches!(
            dio,
            "web"
                | "webrip"
                | "webdl"
                | "bluray"
                | "brrip"
                | "hdrip"
                | "dvdrip"
                | "x264"
                | "x265"
                | "hevc"
                | "h264"
                | "aac"
                | "ac3"
                | "eac3"
                | "proper"
                | "repack"
                | "subs"
                | "sub"
                | "srt"
                | "vtt"
                | "full"
                | "multi"
        )
}

/// Je li datoteka titl?
pub fn is_subtitle_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .map(|ekstenzija| ekstenzija.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default()
            .as_str(),
        "srt" | "vtt" | "ass" | "ssa" | "sub" | "idx" | "smi" | "ttml" | "dfxp"
    )
}

/// Kod kodeka za naslov (iz ekstenzije).
pub fn codec_from_ext(path: &Path) -> String {
    match path
        .extension()
        .map(|ekstenzija| ekstenzija.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
        .as_str()
    {
        "srt" => "srt",
        "vtt" => "vtt",
        "ass" | "ssa" => "ass",
        "sub" => "sub",
        "idx" => "vobsub",
        "smi" => "smi",
        "ttml" | "dfxp" => "ttml",
        _ => "srt",
    }
    .to_string()
}

/// Napravi stazu iz vanjske datoteke, koristeci ime videa za jezik/oznake.
/// Vraca `None` kad datoteka ne pripada tom videu.
pub fn external_track(video_stem: &str, path: &Path) -> Option<SubtitleTrack> {
    let ime = path.file_name()?.to_string_lossy().to_string();
    let bez_ekstenzije = ime.rsplit_once('.').map(|(pred, _)| pred).unwrap_or(&ime);
    let repak = suffix_after_stem(video_stem, bez_ekstenzije)?;
    let (language, forced, sdh) = parse_tokens(&repak);
    // Bez jezika i bez oznake to nije nas titl (`Film.1080p.srt`, `Film.repack.srt`).
    if language.is_none() && !forced && !sdh && !repak.trim().is_empty() {
        return None;
    }
    Some(SubtitleTrack {
        source: Source::External(path.to_path_buf()),
        codec: codec_from_ext(path),
        language,
        label: None,
        forced,
        sdh,
        default: false,
    })
}

/// Dio imena titla nakon imena videa — ali samo uz granicu rijeci, da `Film2.srt`
/// ne pripadne `Film.mkv`.
pub fn suffix_after_stem(video_stem: &str, sub_stem: &str) -> Option<String> {
    let (video, titl) = (video_stem.to_ascii_lowercase(), sub_stem.to_ascii_lowercase());
    if video.is_empty() || !titl.starts_with(&video) {
        return None;
    }
    let ostatak = &sub_stem[video.len()..];
    if ostatak.is_empty() {
        return Some(String::new());
    }
    // Iza imena videa mora ici granica: tacka, crtica, donja crta ili razmak.
    if !ostatak.chars().next().is_some_and(|znak| matches!(znak, '.' | '_' | '-' | ' ')) {
        return None;
    }
    Some(ostatak.trim_start_matches(['.', '_', '-', ' ']).to_string())
}

/// Kod jezika iz bilo kojeg zapisa koji se pojavljuje u praksi.
pub fn language_code(zapis: &str) -> Option<String> {
    let zapis = zapis.trim().to_ascii_lowercase();
    if zapis.is_empty() {
        return None;
    }
    let norma = normaliziraj(&zapis);
    JEZICI
        .iter()
        .find(|(kodovi, ime)| {
            kodovi.iter().any(|kod| *kod == norma)
                || normaliziraj(ime) == norma
                || kodovi.iter().any(|kod| *kod == zapis)
        })
        .and_then(|(kodovi, _)| {
            // Prednost kratkom kodu (`en`, `hr`) — tako se pise i u imenima datoteka.
            kodovi.iter().find(|kod| kod.len() == 2).or_else(|| kodovi.first()).map(|kod| (*kod).to_string())
        })
}

fn normaliziraj(tekst: &str) -> String {
    tekst.chars().filter(|znak| znak.is_ascii_alphabetic()).collect::<String>().to_ascii_lowercase()
}

/// Ime jezika koje korisnik vidi (`English`, `Croatian`…). `und` i smece → `None`.
pub fn language_name(kod: &str) -> Option<&'static str> {
    let kod = normaliziraj(kod);
    if kod.is_empty() || matches!(kod.as_str(), "und" | "unknown" | "none" | "zxx" | "mul") {
        return None;
    }
    JEZICI
        .iter()
        .find(|(kodovi, ime)| kodovi.iter().any(|k| *k == kod) || normaliziraj(ime) == kod)
        .map(|(_, ime)| *ime)
}

/// Kodovi (ISO 639-1 i 639-2/B+639-2/T) uz ime jezika.
#[rustfmt::skip]
pub const JEZICI: &[(&[&str], &str)] = &[
    (&["en", "eng", "english"], "English"),
    (&["hr", "hrv", "scr", "croatian"], "Croatian"),
    (&["sr", "srp", "scc", "serbian"], "Serbian"),
    (&["bs", "bos", "bosnian"], "Bosnian"),
    (&["sl", "slv", "slovenian", "slovene"], "Slovenian"),
    (&["mk", "mkd", "mac", "macedonian"], "Macedonian"),
    (&["bg", "bul", "bulgarian"], "Bulgarian"),
    (&["es", "spa", "spanish", "castellano"], "Spanish"),
    (&["pt", "por", "portuguese"], "Portuguese"),
    (&["fr", "fra", "fre", "french"], "French"),
    (&["de", "deu", "ger", "german"], "German"),
    (&["it", "ita", "italian"], "Italian"),
    (&["nl", "nld", "dut", "dutch"], "Dutch"),
    (&["sv", "swe", "swedish"], "Swedish"),
    (&["no", "nor", "norwegian"], "Norwegian"),
    (&["da", "dan", "danish"], "Danish"),
    (&["fi", "fin", "finnish"], "Finnish"),
    (&["is", "isl", "ice", "icelandic"], "Icelandic"),
    (&["pl", "pol", "polish"], "Polish"),
    (&["cs", "ces", "cze", "czech"], "Czech"),
    (&["sk", "slk", "slo", "slovak"], "Slovak"),
    (&["hu", "hun", "hungarian"], "Hungarian"),
    (&["ro", "ron", "rum", "romanian"], "Romanian"),
    (&["el", "ell", "gre", "greek"], "Greek"),
    (&["tr", "tur", "turkish"], "Turkish"),
    (&["ru", "rus", "russian"], "Russian"),
    (&["uk", "ukr", "ukrainian"], "Ukrainian"),
    (&["be", "bel", "belarusian"], "Belarusian"),
    (&["lt", "lit", "lithuanian"], "Lithuanian"),
    (&["lv", "lav", "latvian"], "Latvian"),
    (&["et", "est", "estonian"], "Estonian"),
    (&["sq", "sqi", "alb", "albanian"], "Albanian"),
    (&["he", "heb", "hebrew", "iw"], "Hebrew"),
    (&["ar", "ara", "arabic"], "Arabic"),
    (&["fa", "fas", "per", "persian", "farsi"], "Persian"),
    (&["hi", "hin", "hindi"], "Hindi"),
    (&["bn", "ben", "bengali"], "Bengali"),
    (&["ta", "tam", "tamil"], "Tamil"),
    (&["te", "tel", "telugu"], "Telugu"),
    (&["ml", "mal", "malayalam"], "Malayalam"),
    (&["ur", "urd", "urdu"], "Urdu"),
    (&["th", "tha", "thai"], "Thai"),
    (&["vi", "vie", "vietnamese"], "Vietnamese"),
    (&["id", "ind", "indonesian"], "Indonesian"),
    (&["ms", "msa", "may", "malay"], "Malay"),
    (&["tl", "tgl", "fil", "tagalog", "filipino"], "Filipino"),
    (&["zh", "zho", "chi", "chinese", "mandarin", "cantonese", "yue"], "Chinese"),
    (&["ja", "jpn", "japanese"], "Japanese"),
    (&["ko", "kor", "korean"], "Korean"),
    (&["ka", "kat", "geo", "georgian"], "Georgian"),
    (&["hy", "hye", "arm", "armenian"], "Armenian"),
    (&["az", "aze", "azerbaijani"], "Azerbaijani"),
    (&["kk", "kaz", "kazakh"], "Kazakh"),
    (&["uz", "uzb", "uzbek"], "Uzbek"),
    (&["sw", "swa", "swahili"], "Swahili"),
    (&["af", "afr", "afrikaans"], "Afrikaans"),
    (&["ca", "cat", "catalan"], "Catalan"),
    (&["gl", "glg", "galician"], "Galician"),
    (&["eu", "eus", "baq", "basque"], "Basque"),
    (&["eo", "epo", "esperanto"], "Esperanto"),
    (&["la", "lat", "latin"], "Latin"),
];

/// Sortiraj staze: tekst prije slika, pa naslovi (`default`, pa abecedno po jeziku).
pub fn poredaj(staze: &mut [SubtitleTrack]) {
    staze.sort_by(|a, b| {
        (
            !a.is_text(),
            !a.default,
            a.forced,
            a.language_name().unwrap_or("zzz"),
            a.language.clone().unwrap_or_default(),
            a.codec.clone(),
        )
            .cmp(&(
                !b.is_text(),
                !b.default,
                b.forced,
                b.language_name().unwrap_or("zzz"),
                b.language.clone().unwrap_or_default(),
                b.codec.clone(),
            ))
    });
}

/// Jezgro bez oznake (kao u `Lanterns`: `subrip`, bez `language`/`title`) — TV bi dobio
/// `Language 1`. Zato jezik pogadjamo iz **teksta titla**: kratki uzorak je dovoljan.
pub fn guess_language(sample: &str) -> Option<&'static str> {
    let uzorak = sample.to_lowercase();
    if uzorak.len() < 60 {
        return None;
    }
    // `ě ř ů` ne postoje u hrvatskom/srpskom/bosanskom — kad ih tekst ima, jezik je
    // ceski ili slovački, bez pogadjanja (dijakritika sama po sebi vara: š/č/ž dijele).
    if uzorak.contains('ě') || uzorak.contains('ř') || uzorak.contains('ů') {
        let slovački = uzorak.contains('ľ') || uzorak.contains('ô') || uzorak.contains('ä');
        return Some(if slovački { "Slovak" } else { "Czech" });
    }

    let rijeci: Vec<String> = uzorak
        .split(|znak: char| !znak.is_alphabetic())
        .filter(|rijec| !rijec.is_empty())
        .map(str::to_string)
        .collect();
    let ima = |rijec: &str| rijeci.iter().any(|kandidat| kandidat == rijec);

    let mut rezultat: Vec<(&'static str, i32)> = Vec::new();

    // Hrvatski/srpski/bosanski: dijakritika + cestice koje se ponavljaju u svakoj recenici.
    let nasa: i32 = ["š", "đ", "č", "ć", "ž"]
        .iter()
        .map(|slovo| uzorak.matches(slovo).count().min(20) as i32)
        .sum::<i32>()
        + ["je", "se", "što", "šta", "nije", "kao", "samo", "ali", "ovo", "sada", "dobro", "ću"]
            .iter()
            .filter(|rijec| ima(rijec))
            .count() as i32
            * 3;
    rezultat.push(("Croatian", nasa));

    let engleski = ["the", "and", "you", "that", "what", "this", "with", "have", "are", "not", "for", "was"]
        .iter()
        .filter(|rijec| ima(rijec))
        .count() as i32
        * 3;
    rezultat.push(("English", engleski));

    let spanski = ["que", "los", "las", "una", "para", "con", "pero", "como", "está", "muy", "no", "es"]
        .iter()
        .filter(|rijec| ima(rijec))
        .count() as i32
        * 3
        + (uzorak.matches('ñ').count().min(10) as i32) * 4
        + (uzorak.matches('¿').count().min(10) as i32) * 4
        + (uzorak.matches('¡').count().min(10) as i32) * 2;
    rezultat.push(("Spanish", spanski));

    let njemacki = ["der", "die", "das", "und", "ist", "nicht", "ich", "sie", "ein", "mit", "wir", "was"]
        .iter()
        .filter(|rijec| ima(rijec))
        .count() as i32
        * 3
        + ["ä", "ö", "ü", "ß"].iter().map(|slovo| uzorak.matches(slovo).count().min(15) as i32).sum::<i32>();
    rezultat.push(("German", njemacki));

    let francuski = ["les", "des", "est", "que", "pour", "dans", "une", "pas", "vous", "nous", "avec"]
        .iter()
        .filter(|rijec| ima(rijec))
        .count() as i32
        * 3
        + ["é", "è", "ê", "ç", "à", "ô"]
            .iter()
            .map(|slovo| uzorak.matches(slovo).count().min(15) as i32)
            .sum::<i32>();
    rezultat.push(("French", francuski));

    let talijanski = ["che", "non", "per", "una", "sono", "mio", "perché", "gli", "anche", "questo"]
        .iter()
        .filter(|rijec| ima(rijec))
        .count() as i32
        * 3;
    rezultat.push(("Italian", talijanski));

    let portugalski = ["não", "uma", "para", "com", "você", "são", "mais", "isso", "muito"]
        .iter()
        .filter(|rijec| ima(rijec))
        .count() as i32
        * 3
        + (uzorak.matches('ã').count().min(10) as i32) * 4;
    rezultat.push(("Portuguese", portugalski));

    let poljski = ["nie", "jest", "się", "jak", "tak", "tylko", "bardzo", "jestem"]
        .iter()
        .filter(|rijec| ima(rijec))
        .count() as i32
        * 3
        + ["ł", "ą", "ę", "ż", "ź"]
            .iter()
            .map(|slovo| uzorak.matches(slovo).count().min(15) as i32)
            .sum::<i32>();
    rezultat.push(("Polish", poljski));

    let turski = ["bir", "ve", "bu", "için", "ile", "çok", "değil"].iter().filter(|rijec| ima(rijec)).count()
        as i32
        * 3
        + ["ğ", "ş", "ı"].iter().map(|slovo| uzorak.matches(slovo).count().min(15) as i32).sum::<i32>();
    rezultat.push(("Turkish", turski));

    rezultat.sort_by(|a, b| b.1.cmp(&a.1));
    let (ime, bodovi) = rezultat[0];
    // Bez jasnog pobjednika (npr. samo imena likova u titlu) bolje ne tvrditi nista.
    if bodovi < 6 {
        return None;
    }
    Some(ime)
}

/// Izvuci ugradjenu stazu titla u SRT (za posluzivanje i za pogadjanje jezika).
pub fn extract_track(video: &Path, index: u32, ffmpeg: &str, izlaz: &Path) -> bool {
    if izlaz.metadata().map(|meta| meta.len() > 0).unwrap_or(false) {
        return true;
    }
    if let Some(mapa) = izlaz.parent() {
        std::fs::create_dir_all(mapa).ok();
    }
    let ishod = std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
        .arg(video)
        .args(["-map", &format!("0:{index}"), "-f", "srt"])
        .arg(izlaz)
        .output();
    matches!(ishod, Ok(izhod) if izhod.status.success())
        && izlaz.metadata().map(|meta| meta.len() > 0).unwrap_or(false)
}

/// Ugradjenim stazama bez jezika pogodi jezik iz teksta titla (radi u pozadini).
pub fn fill_languages(staze: &mut [SubtitleTrack], video: &Path, ffmpeg: &str) -> usize {
    let mut pogodjeno = 0;
    for staza in staze.iter_mut() {
        if staza.language.is_some() || !staza.is_text() {
            continue;
        }
        let Source::Embedded { index } = staza.source else {
            continue;
        };
        let uzorak_putanja = std::env::temp_dir()
            .join("rustiio-jezik")
            .join(format!("{}-{index}.srt", video.file_name().unwrap_or_default().to_string_lossy()));
        if !extract_track(video, index, ffmpeg, &uzorak_putanja) {
            continue;
        }
        let Ok(tekst) = std::fs::read_to_string(&uzorak_putanja) else {
            continue;
        };
        let uzorak: String = tekst.chars().take(200_000).collect();
        if let Some(ime) = guess_language(&uzorak) {
            staza.language = Some(ime.to_ascii_lowercase());
            pogodjeno += 1;
        }
    }
    pogodjeno
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vanjski(video: &str, ime: &str) -> Option<SubtitleTrack> {
        external_track(video, &PathBuf::from("/media").join(ime))
    }

    #[test]
    fn jezik_se_cita_iz_koda_i_imena() {
        assert_eq!(language_name("en"), Some("English"));
        assert_eq!(language_name("ENG"), Some("English"));
        assert_eq!(language_name("English"), Some("English"));
        assert_eq!(language_name("spa"), Some("Spanish"));
        assert_eq!(language_name("es"), Some("Spanish"));
        assert_eq!(language_name("hrv"), Some("Croatian"));
        assert_eq!(language_name("hr"), Some("Croatian"));
        assert_eq!(language_name("deu"), Some("German"));
        assert_eq!(language_name("ger"), Some("German"));
        assert_eq!(language_name("und"), None);
        assert_eq!(language_name(""), None);
    }

    #[test]
    fn titl_uz_video_bez_jezika_ima_prazan_repak() {
        let staza = vanjski("Lanterns.S01E06", "Lanterns.S01E06.srt").expect("titl");
        assert_eq!(staza.language, None);
        assert_eq!(staza.display_name(), "srt");
    }

    #[test]
    fn jezik_i_oznake_iz_imena() {
        let staza = vanjski("Film", "Film.en.srt").expect("titl");
        assert_eq!(staza.language.as_deref(), Some("en"));
        assert_eq!(staza.display_name(), "English");

        let staza = vanjski("Film", "Film.hrv.forced.srt").expect("titl");
        assert_eq!(staza.language_name(), Some("Croatian"));
        assert!(staza.forced);
        assert_eq!(staza.display_name(), "Croatian (forced)");

        let staza = vanjski("Film", "Film.eng.SDH.srt").expect("titl");
        assert_eq!(staza.language_name(), Some("English"));
        assert!(staza.sdh);
        assert_eq!(staza.display_name(), "English (SDH)");

        let staza = vanjski("Film", "Film.Spanish.srt").expect("titl");
        assert_eq!(staza.language_name(), Some("Spanish"));
    }

    #[test]
    fn slicna_imena_i_tehnicki_repak_se_ne_lijepe() {
        assert!(vanjski("Film", "Film2.srt").is_none(), "Film2 nije Film");
        assert!(vanjski("Film", "Film.1080p.srt").is_none(), "1080p nije jezik");
        assert!(vanjski("Film", "Film.webrip.srt").is_none(), "webrip nije jezik");
        assert!(vanjski("Film", "Drugi.srt").is_none());
        assert!(vanjski("Film", "Film.en.forced.srt").is_some());
    }

    #[test]
    fn slikovni_titl_nije_tekst() {
        let mut staza = vanjski("Film", "Film.en.srt").expect("titl");
        assert!(staza.is_text());
        staza.codec = "hdmv_pgs_subtitle".to_string();
        assert!(!staza.is_text(), "PGS se ne moze posluziti kao srt");
        staza.codec = "vobsub".to_string();
        assert!(!staza.is_text());
        staza.codec = "sub".to_string();
        assert!(staza.is_text(), "MicroDVD se moze prevesti u srt");
    }

    #[test]
    fn ugradjena_staza_dobiva_ime_za_url() {
        let staza = SubtitleTrack {
            source: Source::Embedded { index: 3 },
            codec: "subrip".to_string(),
            language: Some("hrv".to_string()),
            label: Some("SDH".to_string()),
            forced: false,
            sdh: true,
            default: true,
        };
        assert_eq!(staza.serve_name("Lanterns.S01E06"), "Lanterns.S01E06.hrv.srt");
        assert_eq!(staza.display_name(), "Croatian (SDH)");
        assert_eq!(staza.path(), None);
    }

    #[test]
    fn jezik_se_pogadja_iz_teksta_titla() {
        let hrvatski = "1\n00:00:01,000 --> 00:00:03,000\nŠto je ovo? Nije dobro, ali samo trenutak.\n\n2\n00:00:04,000 --> 00:00:06,000\nSada ću ti reći što se dogodilo jer se bojim.\n";
        assert_eq!(guess_language(hrvatski), Some("Croatian"));

        let engleski = "1\n00:00:01,000 --> 00:00:03,000\nWhat is this? That is not good, and you know it.\n\n2\n00:00:04,000 --> 00:00:06,000\nI have to tell you what happened, because this is not over.\n";
        assert_eq!(guess_language(engleski), Some("English"));

        let spanski = "1\n00:00:01,000 --> 00:00:03,000\n¿Qué es esto? No está bien, pero los niños están aquí.\n\n2\n00:00:04,000 --> 00:00:06,000\nTengo que decirte lo que pasó con una señora muy simpática.\n";
        assert_eq!(guess_language(spanski), Some("Spanish"));

        let njemacki = "1\n00:00:01,000 --> 00:00:03,000\nWas ist das? Das ist nicht gut und ich weiß es.\n\n2\n00:00:04,000 --> 00:00:06,000\nWir müssen gehen, aber sie ist noch hier mit dem Auto.\n";
        assert_eq!(guess_language(njemacki), Some("German"));

        let ceski = "1\n00:00:01,000 --> 00:00:03,000\nVím, že to zní šíleně, ale musím ti to říct.\n\n2\n00:00:04,000 --> 00:00:06,000\nKdyž jsem byl malý, říkali mi, že se to nedá.\n";
        assert_eq!(guess_language(ceski), Some("Czech"), "ceski ne smije proci kao hrvatski");

        // Prekratak uzorak ili bez signala — bolje ne tvrditi nista.
        assert_eq!(guess_language("Zdravo"), None);
        assert_eq!(guess_language("1\n00:00:01,000 --> 00:00:02,000\n...\n"), None);
    }

    #[test]
    fn sortiranje_stavlja_tekst_i_default_naprijed() {
        let mut staze = vec![
            SubtitleTrack {
                source: Source::Embedded { index: 5 },
                codec: "hdmv_pgs_subtitle".to_string(),
                language: Some("eng".to_string()),
                label: None,
                forced: false,
                sdh: false,
                default: false,
            },
            SubtitleTrack {
                source: Source::Embedded { index: 1 },
                codec: "subrip".to_string(),
                language: Some("spa".to_string()),
                label: None,
                forced: false,
                sdh: false,
                default: false,
            },
            SubtitleTrack {
                source: Source::Embedded { index: 2 },
                codec: "subrip".to_string(),
                language: Some("hrv".to_string()),
                label: None,
                forced: false,
                sdh: false,
                default: true,
            },
        ];
        poredaj(&mut staze);
        assert_eq!(staze[0].language_name(), Some("Croatian"), "default ide prvi");
        assert_eq!(staze[1].language_name(), Some("Spanish"));
        assert!(!staze[2].is_text(), "slikovni ide na kraj");
    }
}
