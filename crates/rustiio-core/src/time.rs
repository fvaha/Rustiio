//! Formatiranje vremena bez vanjskih ovisnosti (dovoljno za `dc:date`,
//! `Last-Modified` i trajanje u DIDL-u).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `2026-09-24T09:36:46` iz `SystemTime` (UTC).
pub fn format_rfc3339(time: SystemTime) -> String {
    let secs = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => 0,
    };
    format_epoch(secs)
}

/// Isto, ali iz Unix sekundi.
pub fn format_epoch(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}")
}

/// `H:MM:SS.mmm` — format koji DLNA ocekuje u `res@duration`.
pub fn format_duration(d: Duration) -> String {
    let total_ms = d.as_millis() as u64;
    let ms = total_ms % 1000;
    let total = total_ms / 1000;
    format!("{}:{:02}:{:02}.{:03}", total / 3600, (total % 3600) / 60, total % 60, ms)
}

/// Howard Hinnant: dani od 1970-01-01 -> (godina, mjesec, dan).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_formats_known_dates() {
        assert_eq!(format_epoch(0), "1970-01-01T00:00:00");
        assert_eq!(format_epoch(1_000_000_000), "2001-09-09T01:46:40");
        // Prestupna godina i kraj mjeseca.
        assert_eq!(format_epoch(1_709_164_800), "2024-02-29T00:00:00");
    }

    #[test]
    fn duration_format_matches_dlna_shape() {
        assert_eq!(format_duration(Duration::from_millis(0)), "0:00:00.000");
        assert_eq!(format_duration(Duration::from_millis(1_500)), "0:00:01.500");
        assert_eq!(format_duration(Duration::from_millis(7_322_000)), "2:02:02.000");
    }

    #[test]
    fn rfc3339_from_system_time() {
        // Referentna vrijednost provjerena s `date -u -r 1756924800` = 2025-09-03T18:40:00.
        let t = UNIX_EPOCH + Duration::from_secs(1_756_924_800);
        assert_eq!(format_rfc3339(t), "2025-09-03T18:40:00");
    }
}
