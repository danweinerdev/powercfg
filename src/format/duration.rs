//! Human-readable duration formatting.
//!
//! Mirrors the Python `format_duration` (`powercfg.py` lines 25-42)
//! byte-for-byte. The rule: emit each non-zero component (`d`/`h`/`m`/`s`)
//! separated by a single space, and always emit the seconds component if
//! every other component is zero (so `Duration::ZERO` renders as `"0s"`,
//! not the empty string).

use std::time::Duration;

/// Format a [`Duration`] as `"1d 2h 3m 4s"`, omitting zero leading
/// components.
///
/// Sub-second precision is intentionally truncated: the Python source
/// uses `int(td.total_seconds())` and downstream renderers (sleep
/// duration, idle time) display whole seconds.
// TODO(phase-2/4): consumed by cmd::energy (CPU idle) and cmd::lastwake
// (sleep/wake duration). Drop the allow when either lands.
#[allow(dead_code)]
pub fn format_duration(d: Duration) -> String {
    let total_seconds = d.as_secs();
    let days = total_seconds / 86_400;
    let hours = (total_seconds % 86_400) / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;

    let mut parts: Vec<String> = Vec::with_capacity(4);
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{seconds}s"));
    }

    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_renders_as_zero_seconds() {
        assert_eq!(format_duration(Duration::ZERO), "0s");
    }

    #[test]
    fn under_a_minute_is_seconds_only() {
        assert_eq!(format_duration(Duration::from_secs(59)), "59s");
    }

    #[test]
    fn whole_minute_omits_zero_seconds() {
        assert_eq!(format_duration(Duration::from_secs(60)), "1m");
    }

    #[test]
    fn whole_hour_omits_zero_minutes_and_seconds() {
        assert_eq!(format_duration(Duration::from_secs(3600)), "1h");
    }

    #[test]
    fn one_hour_one_minute_one_second() {
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h 1m 1s");
    }

    #[test]
    fn whole_day_omits_lower_components() {
        assert_eq!(format_duration(Duration::from_secs(86_400)), "1d");
    }

    #[test]
    fn day_hour_minute_second() {
        assert_eq!(format_duration(Duration::from_secs(90_061)), "1d 1h 1m 1s");
    }

    #[test]
    fn multi_week_duration() {
        // 14 days, 2 hours, 30 minutes, 5 seconds.
        let d = Duration::from_secs(14 * 86_400 + 2 * 3_600 + 30 * 60 + 5);
        assert_eq!(format_duration(d), "14d 2h 30m 5s");
    }

    #[test]
    fn sub_second_precision_is_truncated() {
        // 1.999 seconds rounds down to 1s, matching Python's int(...) cast.
        let d = Duration::from_millis(1_999);
        assert_eq!(format_duration(d), "1s");
    }
}
