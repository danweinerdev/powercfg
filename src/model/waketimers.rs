//! Owning data structures for the `waketimers` subcommand.
//!
//! `WakeTimersReport` is the top-level handler-built bundle; `TimerEntry`
//! is the per-unit leaf produced by `source::dbus::list_systemd_timers`.

use serde::Serialize;

/// One systemd timer unit, queried via
/// `org.freedesktop.systemd1.Manager.ListUnits` plus per-unit `Timer`
/// interface property reads.
///
/// `next_elapse_realtime_us` is microseconds since the Unix epoch
/// (systemd's `CLOCK_REALTIME`). `0` indicates "no next elapse" (timer
/// disabled or hasn't computed yet); `u64::MAX` is the systemd "no next"
/// sentinel for some monotonic timers leaking through. The
/// [`TimerEntry::next_elapse_display`] helper renders both as `"n/a"`.
///
/// JSON contract: the design schema names the wake-flag field `"wakes"`
/// for `all_timers` rows, but here we expose `wake_system` (matching
/// the systemd property name) on every row. The `next` field in the
/// schema sketch is an already-rendered display string; this struct
/// instead exposes `next_elapse_realtime_us` (raw µs since epoch) plus
/// a separate display helper. Documented as deviation in
/// `docs/json-schema.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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
    /// Uses `chrono::Local`, which on Linux delegates to
    /// `iana-time-zone` reading `/etc/localtime` (NOT `$TZ`). This
    /// matches the Python tool's `datetime.fromtimestamp` semantics
    /// in spirit but diverges in one detail: the `%Z` specifier
    /// renders as a numeric offset (`+00:00`, `-07:00`) when
    /// `iana-time-zone` can't resolve a name, where Python always
    /// produces a zone abbreviation (`UTC`, `PDT`). Both are valid
    /// timestamps; users on most desktops will see numeric. If
    /// abbreviation parity becomes important, pulling in `chrono-tz`
    /// would resolve named zones — not a current dependency.
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
/// `all_timers` is only populated in verbose mode and capped at 15
/// rows when rendered.
///
/// JSON contract:
/// - `wake_timers` is always an array (never omitted).
/// - `all_timers` is `None` when `--verbose` was not set and is
///   omitted from JSON; with `--verbose` it serializes as an array
///   (possibly empty).
/// - `rtc_wakealarm` serializes as `null` when absent — preserves
///   "no alarm scheduled / source unavailable".
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WakeTimersReport {
    pub wake_timers: Vec<TimerEntry>,
    /// `None` when `--verbose` was not set; the JSON key is omitted
    /// in that case so consumers can distinguish "didn't list all
    /// timers" from "listed and found none".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_timers: Option<Vec<TimerEntry>>,
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

    /// Default report (no verbose, no RTC alarm): `wake_timers` is an
    /// empty array, `all_timers` is omitted, `rtc_wakealarm` is null.
    #[test]
    fn waketimers_report_default_renders_expected_shape() {
        let report = WakeTimersReport::default();
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(v["wake_timers"].is_array());
        assert_eq!(v["wake_timers"].as_array().unwrap().len(), 0);
        // `all_timers: None` → key omitted (verbose-gated).
        assert!(!v.as_object().unwrap().contains_key("all_timers"));
        // `rtc_wakealarm: None` → null.
        assert!(v["rtc_wakealarm"].is_null());
    }

    /// Verbose report with empty `all_timers` Vec: key present as `[]`.
    /// This locks the `null` (omitted) vs `[]` (present-and-empty)
    /// distinction.
    #[test]
    fn waketimers_report_verbose_empty_renders_array_not_omitted() {
        let report = WakeTimersReport {
            wake_timers: vec![],
            all_timers: Some(vec![]),
            rtc_wakealarm: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["all_timers"].is_array());
        assert_eq!(v["all_timers"].as_array().unwrap().len(), 0);
    }

    /// Populated report: TimerEntry rows render with snake_case keys
    /// (`unit`, `wake_system`, `next_elapse_realtime_us`).
    #[test]
    fn waketimers_report_populated_renders_timer_keys() {
        let timer = TimerEntry {
            unit: "snapshot.timer".into(),
            wake_system: true,
            next_elapse_realtime_us: 1_745_939_400_000_000,
        };
        let report = WakeTimersReport {
            wake_timers: vec![timer.clone()],
            all_timers: Some(vec![timer]),
            rtc_wakealarm: Some("2026-04-29 06:00:00".into()),
        };
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(v["wake_timers"][0]["unit"], "snapshot.timer");
        assert_eq!(v["wake_timers"][0]["wake_system"], true);
        assert_eq!(
            v["wake_timers"][0]["next_elapse_realtime_us"],
            1_745_939_400_000_000_u64,
        );
        assert_eq!(v["rtc_wakealarm"], "2026-04-29 06:00:00");
    }
}
