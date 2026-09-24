//! Parsiranje `Range: bytes=...` headera (ukljucivo, kao u HTTP specifikaciji).

/// Parsiraj `bytes=` range i vrati ukljucivi `(start, end)` za datoteku velicine `total`.
///
/// Vraca `None` kada range treba ignorirati ili odbiti s 416:
/// prazna/nesintaksna vrijednost, range izvan datoteke, obrnut range.
pub fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let spec = header.trim();
    let spec = spec
        .strip_prefix("bytes=")
        .or_else(|| spec.strip_prefix("BYTES="))
        .or_else(|| spec.strip_prefix("Bytes="))?;
    // Uzimamo samo prvi range; multi-range TV-i ne koriste (a mi ga ne podrzavamo).
    let first = spec.split(',').next()?.trim();
    let (start_raw, end_raw) = first.split_once('-')?;
    let (start_raw, end_raw) = (start_raw.trim(), end_raw.trim());

    if start_raw.is_empty() {
        // Suffix range: `bytes=-500` -> zadnjih 500 bajtova.
        let suffix: u64 = end_raw.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        let len = suffix.min(total);
        return Some((total - len, total - 1));
    }

    let start: u64 = start_raw.parse().ok()?;
    if start >= total {
        return None;
    }
    let end = if end_raw.is_empty() { total - 1 } else { end_raw.parse::<u64>().ok()?.min(total - 1) };
    if end < start {
        return None;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_ended_range_runs_to_end() {
        assert_eq!(parse_range("bytes=0-", 1000), Some((0, 999)));
        assert_eq!(parse_range("bytes=100-", 1000), Some((100, 999)));
    }

    #[test]
    fn closed_range_is_inclusive() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=500-999", 1000), Some((500, 999)));
    }

    #[test]
    fn suffix_range_takes_last_bytes() {
        assert_eq!(parse_range("bytes=-500", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=-5000", 1000), Some((0, 999)));
    }

    #[test]
    fn end_beyond_file_is_clamped() {
        assert_eq!(parse_range("bytes=500-100000", 1000), Some((500, 999)));
    }

    #[test]
    fn invalid_ranges_are_rejected() {
        assert_eq!(parse_range("bytes=1000-", 1000), None, "start izvan datoteke");
        assert_eq!(parse_range("bytes=500-100", 1000), None, "obrnut range");
        assert_eq!(parse_range("bytes=-0", 1000), None);
        assert_eq!(parse_range("items=0-10", 1000), None, "pogresna jedinica");
        assert_eq!(parse_range("bytes=abc-def", 1000), None);
        assert_eq!(parse_range("bytes=0-10", 0), None, "prazna datoteka");
    }

    #[test]
    fn multi_range_uses_first_part() {
        assert_eq!(parse_range("bytes=0-9,20-29", 1000), Some((0, 9)));
    }
}
