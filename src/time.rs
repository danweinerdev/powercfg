//! ISO-8601 timestamp parsing for journalctl output.
//!
//! Mirrors the Python tool's `parse_iso_timestamp` (`powercfg.py` lines
//! 14-22) which accepts both `±HH:MM` (`-08:00`) and `±HHMM` (`-0800`)
//! offsets. Returns `Option` rather than `Result` because callers treat
//! parse failures as "skip this line and continue" — the journal is
//! best-effort.

use chrono::{DateTime, FixedOffset};

/// Parse an ISO-8601 timestamp with a numeric timezone offset.
///
/// Accepts both `2025-12-25T21:40:21-08:00` (`%:z`) and
/// `2025-12-25T21:40:21-0800` (`%z`). Rejects `Z` (UTC marker), naive
/// timestamps without an offset, and any other malformed input by
/// returning `None`. The Python source likewise does not handle `Z`.
pub fn parse_iso_timestamp(s: &str) -> Option<DateTime<FixedOffset>> {
    // Try the colon-separated form first (the more common journalctl
    // shape: `short-iso` emits `±HH:MM`).
    DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%:z")
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn parses_offset_with_colon() {
        let dt = parse_iso_timestamp("2025-12-25T21:40:21-08:00").expect("parse");
        assert_eq!(dt.year(), 2025);
        assert_eq!(dt.month(), 12);
        assert_eq!(dt.day(), 25);
        assert_eq!(dt.hour(), 21);
        assert_eq!(dt.minute(), 40);
        assert_eq!(dt.second(), 21);
        assert_eq!(dt.offset().local_minus_utc(), -8 * 3600);
    }

    #[test]
    fn parses_offset_without_colon() {
        let dt = parse_iso_timestamp("2025-12-25T21:40:21-0800").expect("parse");
        assert_eq!(dt.offset().local_minus_utc(), -8 * 3600);
    }

    #[test]
    fn parses_positive_offset_with_colon() {
        let dt = parse_iso_timestamp("2026-04-29T00:00:00+05:30").expect("parse");
        assert_eq!(
            dt.offset().local_minus_utc(),
            5 * 3600 + 30 * 60,
            "offset should be +05:30 in seconds",
        );
    }

    #[test]
    fn parses_zero_offset_with_colon() {
        let dt = parse_iso_timestamp("2026-01-01T00:00:00+00:00").expect("parse");
        assert_eq!(dt.offset().local_minus_utc(), 0);
    }

    #[test]
    fn rejects_z_suffix() {
        // The Python tool only handles ±HH:MM / ±HHMM; reject Z cleanly so
        // callers don't silently accept a different shape.
        assert!(parse_iso_timestamp("2025-12-25T21:40:21Z").is_none());
    }

    #[test]
    fn rejects_naive_timestamp() {
        assert!(parse_iso_timestamp("2025-12-25T21:40:21").is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_iso_timestamp("").is_none());
        assert!(parse_iso_timestamp("not a timestamp").is_none());
        assert!(parse_iso_timestamp("2025-13-25T21:40:21-08:00").is_none());
        assert!(parse_iso_timestamp("2025-12-25 21:40:21-08:00").is_none());
    }
}
