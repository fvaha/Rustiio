//! `TimeSeekRange.dlna.org` — seek po vremenu, ne po bajtovima.
//!
//! Neki Samsung/Philips modeli ne salju `Range: bytes=...` nego traze "od 14:20
//! do 20:00" i ocekuju da im server vrati bajtove koji odgovaraju tom vremenu.
//! Zato nam treba trajanje filma (vidi `rustiio_library::DurationProbe`).

/// Trazeno vremensko razdoblje u milisekundama.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSeek {
    pub start_ms: u64,
    /// `None` = "do kraja filma".
    pub end_ms: Option<u64>,
}

/// Parsiraj `TimeSeekRange.dlna.org` header (`npt=00:14:20-00:20:00`, `npt=860-`,
/// `npt=00:14:20.500-`). Vraca `None` kad header nije upotrebljiv — tada se seek
/// ignorira i posluzi se cijeli film (sto je TV-u valjan odgovor).
pub fn parse_time_seek(header: &str) -> Option<TimeSeek> {
    let value = header.trim();
    // Prihvatamo i s prefiksom (`npt=`) i bez njega.
    let value = match value.split_once('=') {
        Some((key, rest)) if key.trim().eq_ignore_ascii_case("npt") => rest.trim(),
        Some((_, rest)) => rest.trim(),
        None => value,
    };
    let (start_raw, end_raw) = value.split_once('-')?;
    let start_ms = parse_npt_time(start_raw)?;
    let end_ms = if end_raw.trim().is_empty() { None } else { Some(parse_npt_time(end_raw)?) };
    if let Some(end) = end_ms {
        if end < start_ms {
            return None;
        }
    }
    Some(TimeSeek { start_ms, end_ms })
}

/// `00:14:20.500`, `14:20`, `860` ili `860.5` -> milisekunde.
pub fn parse_npt_time(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let parts: Vec<&str> = raw.split(':').collect();
    let (hours, minutes, seconds) = match parts.len() {
        1 => (0u64, 0u64, parts[0]),
        2 => (0, parts[0].trim().parse().ok()?, parts[1]),
        3 => (parts[0].trim().parse().ok()?, parts[1].trim().parse().ok()?, parts[2]),
        _ => return None,
    };
    let seconds: f64 = seconds.trim().parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(hours * 3_600_000 + minutes * 60_000 + (seconds * 1000.0).round() as u64)
}

/// Milisekunde -> `H:MM:SS.mmm` (format koji DLNA ocekuje u odgovoru).
pub fn format_npt(ms: u64) -> String {
    let total_seconds = ms / 1000;
    format!(
        "{}:{:02}:{:02}.{:03}",
        total_seconds / 3600,
        (total_seconds % 3600) / 60,
        total_seconds % 60,
        ms % 1000
    )
}

/// Pretvori vrijeme u bajt (linearna procjena po trajanju — dovoljno za seek).
pub fn byte_for_time(ms: u64, duration_ms: u64, total: u64) -> u64 {
    if duration_ms == 0 || total == 0 {
        return 0;
    }
    let offset = (ms as u128 * total as u128) / duration_ms as u128;
    offset.min(total as u128) as u64
}

/// Vrijednost `TimeSeekRange.dlna.org` headera u odgovoru.
pub fn response_header(start_ms: u64, end_ms: u64, duration_ms: u64) -> String {
    format!("npt={}-{}/{}", format_npt(start_ms), format_npt(end_ms), format_npt(duration_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hms_range() {
        assert_eq!(
            parse_time_seek("npt=00:14:20-00:20:00"),
            Some(TimeSeek { start_ms: 860_000, end_ms: Some(1_200_000) })
        );
    }

    #[test]
    fn parses_open_ended_and_short_forms() {
        assert_eq!(parse_time_seek("npt=860-"), Some(TimeSeek { start_ms: 860_000, end_ms: None }));
        assert_eq!(parse_time_seek("npt=14:20-"), Some(TimeSeek { start_ms: 860_000, end_ms: None }));
        assert_eq!(parse_time_seek("npt=20.5-30"), Some(TimeSeek { start_ms: 20_500, end_ms: Some(30_000) }));
        assert_eq!(parse_time_seek("00:00:05-"), Some(TimeSeek { start_ms: 5_000, end_ms: None }));
    }

    #[test]
    fn rejects_broken_headers() {
        assert_eq!(parse_time_seek("npt="), None);
        assert_eq!(parse_time_seek("npt=abc-def"), None);
        assert_eq!(parse_time_seek("npt=00:20:00-00:14:20"), None, "obrnut range");
        assert_eq!(parse_time_seek("npt=1:2:3:4-"), None);
    }

    #[test]
    fn formats_npt_like_dlna_expects() {
        assert_eq!(format_npt(0), "0:00:00.000");
        assert_eq!(format_npt(860_000), "0:14:20.000");
        assert_eq!(format_npt(3_723_456), "1:02:03.456");
    }

    #[test]
    fn byte_offset_is_linear_and_clamped() {
        assert_eq!(byte_for_time(0, 100_000, 1_000), 0);
        assert_eq!(byte_for_time(50_000, 100_000, 1_000), 500);
        assert_eq!(byte_for_time(100_000, 100_000, 1_000), 1000, "kraj je duljina, ne duljina-1");
        assert_eq!(byte_for_time(200_000, 100_000, 1_000), 1000, "ne prelazi velicinu");
        assert_eq!(byte_for_time(1, 0, 1_000), 0, "bez trajanja nema seeka");
    }

    #[test]
    fn response_header_shape() {
        assert_eq!(response_header(860_000, 1_200_000, 7_200_000), "npt=0:14:20.000-0:20:00.000/2:00:00.000");
    }
}
