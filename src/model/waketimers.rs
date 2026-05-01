//! Owning data structures for the `waketimers` subcommand.
//!
//! `WakeTimersReport` is the top-level handler-built bundle; `TimerEntry`
//! is the per-unit leaf produced by `source::dbus::list_systemd_timers`.

/// One systemd timer unit, queried via
/// `org.freedesktop.systemd1.Manager.ListUnits` plus per-unit `Timer`
/// interface property reads.
///
/// `next_elapse_realtime_us` is microseconds since the Unix epoch
/// (systemd's `CLOCK_REALTIME`). `0` indicates "no next elapse" (timer
/// disabled or hasn't computed yet); `u64::MAX` is the systemd "no next"
/// sentinel for some monotonic timers leaking through. The
/// [`TimerEntry::next_elapse_display`] helper renders both as `"n/a"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerEntry {
    /// Unit name, always ends in `.timer`.
    pub unit: String,
    /// `WakeSystem` property — `true` if firing this timer wakes the
    /// machine from suspend (RTC-backed).
    pub wake_system: bool,
    /// `NextElapseUSecRealtime` property in microseconds since the Unix
    /// epoch. Sentinels: `0` = unscheduled, `u64::MAX` = no next.
    pub next_elapse_realtime_us: u64,
}

impl TimerEntry {
    /// Convert the µs-since-epoch `next_elapse_realtime_us` into a
    /// human-readable local-time string formatted as
    /// `YYYY-MM-DD HH:MM:SS TZ`, or `"n/a"` for the systemd sentinels
    /// (`0` = unscheduled, `u64::MAX` = "no next").
    ///
    /// Uses `chrono::Local` to match the Python tool's
    /// `datetime.fromtimestamp` behavior — formatting is locale/TZ
    /// dependent. Snapshot tests force `TZ=UTC` for stability.
    pub fn next_elapse_display(&self) -> String {
        const MAX: u64 = u64::MAX;
        match self.next_elapse_realtime_us {
            0 | MAX => "n/a".to_string(),
            us => {
                use chrono::TimeZone;
                let secs = (us / 1_000_000) as i64;
                let nsec = ((us % 1_000_000) * 1_000) as u32;
                match chrono::Local.timestamp_opt(secs, nsec).single() {
                    Some(dt) => dt.format("%Y-%m-%d %H:%M:%S %Z").to_string(),
                    None => "n/a".to_string(),
                }
            }
        }
    }
}

/// Top-level data for the `waketimers` subcommand. Built by
/// `cmd::waketimers::run` from the D-Bus timer walk plus the RTC alarm.
///
/// `wake_timers` are the subset of `all_timers` where `wake_system` is
/// true. They're stored separately because the printer treats them
/// differently (always shown, with a "no wake timers active" fallback);
/// `all_timers` is only rendered in verbose mode and capped at 15 rows.
#[derive(Debug, Default)]
pub struct WakeTimersReport {
    pub wake_timers: Vec<TimerEntry>,
    pub all_timers: Vec<TimerEntry>,
    /// Formatted local-time string from `/sys/class/rtc/rtc0/wakealarm`.
    /// `None` means no alarm scheduled (or the file is unreadable —
    /// the caller swallows the error).
    pub rtc_wakealarm: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(us: u64) -> TimerEntry {
        TimerEntry {
            unit: "test.timer".to_string(),
            wake_system: false,
            next_elapse_realtime_us: us,
        }
    }

    #[test]
    fn next_elapse_display_zero_renders_na() {
        assert_eq!(entry(0).next_elapse_display(), "n/a");
    }

    #[test]
    fn next_elapse_display_max_renders_na() {
        assert_eq!(entry(u64::MAX).next_elapse_display(), "n/a");
    }

    #[test]
    fn next_elapse_display_real_us_matches_format() {
        // 1745939400_000000 µs == 2025-04-29 ~ish. Don't pin the exact
        // date string — chrono::Local depends on the runtime TZ, and
        // tests run on developer/CI machines with arbitrary TZ. Just
        // check the YYYY-MM-DD HH:MM:SS prefix shape and that a
        // timezone abbreviation follows.
        let s = entry(1_745_939_400_000_000).next_elapse_display();
        assert_ne!(s, "n/a", "real µs should not render as n/a: {s}");
        // First 19 chars should look like "YYYY-MM-DD HH:MM:SS".
        let prefix: String = s.chars().take(19).collect();
        assert_eq!(prefix.len(), 19, "expected 19-char prefix in {s:?}");
        let bytes = prefix.as_bytes();
        for (i, b) in bytes.iter().enumerate() {
            let ok = match i {
                4 | 7 => *b == b'-',
                10 => *b == b' ',
                13 | 16 => *b == b':',
                _ => b.is_ascii_digit(),
            };
            assert!(
                ok,
                "char {i} of {prefix:?} doesn't match YYYY-MM-DD HH:MM:SS",
            );
        }
        // After the timestamp + space, expect a non-empty TZ token.
        assert!(
            s.len() > 20,
            "expected timezone suffix after timestamp: {s:?}",
        );
    }
}
