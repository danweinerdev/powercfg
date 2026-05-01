//! Kernel-journal queries via `journalctl`.
//!
//! Replaces the Python tool's `shell=True` pipelines (`journalctl ... |
//! grep ... | tail -1`) with explicit subprocess invocation through
//! `source::exec` and in-process line filtering. Decision 3 in the
//! design names this layer's contract.
//!
//! `journalctl` is the canonical interface for systemd journal queries
//! (no clean library equivalent, and the systemd journal API itself is
//! a moving target). The captured output is typically a few KB even
//! for 30 days of suspend events because we filter at the kernel level
//! via `-k`, but the threaded pipe-drain in `source::exec` handles
//! larger payloads safely.
//!
//! ## Test seam
//!
//! Setting `POWERCFG_JOURNAL_FIXTURE` to a captured-output file path
//! makes [`run_journalctl`] read that file instead of spawning a
//! subprocess. The seam mirrors `POWERCFG_SYSROOT` for sysfs reads.
//! Production callers never set the env var; only integration tests do.

use std::process::Command;
use std::time::Duration;

use chrono::{DateTime, FixedOffset};

use crate::model::lastwake::{SleepEvent, SleepEventKind};
use crate::source::SourceError;
use crate::source::exec::run_with_timeout;
use crate::time::parse_iso_timestamp;

/// Environment variable that, when set, overrides the live `journalctl`
/// invocation with a read of the named file. See module docs.
const JOURNAL_FIXTURE_ENV: &str = "POWERCFG_JOURNAL_FIXTURE";

/// Run `journalctl -k -o short-iso --no-pager --since <since>` with a
/// 15-second timeout, capture stdout, and return it as a UTF-8 string.
///
/// In fixture mode (env var set), `since` is ignored — the fixture
/// file is the entire window, and the helpers above filter it the same
/// way they filter live output.
fn run_journalctl(since: &str) -> Result<String, SourceError> {
    // Test seam: integration tests set this env var to a captured
    // `journalctl` output file. Production never sets it, so the live
    // path is the default. Same pattern as POWERCFG_SYSROOT in
    // `paths::SysRoot`.
    if let Some(path) = std::env::var_os(JOURNAL_FIXTURE_ENV) {
        let _ = since; // ignored in fixture mode
        return std::fs::read_to_string(&path).map_err(SourceError::Io);
    }
    let mut cmd = Command::new("journalctl");
    cmd.args(["-k", "-o", "short-iso", "--no-pager", "--since", since]);
    let output = run_with_timeout(cmd, Duration::from_secs(15))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Find the most recent journal line whose body contains `matcher` and
/// whose head parses as an ISO-8601 timestamp. Returns `Ok(None)` when
/// no matching line is found in the captured window.
///
/// Default callers use `since = "7 days ago"` to match Python's
/// behavior for `lastwake` (no `-n N`) — see `powercfg.py` lines 247
/// and 269.
// TODO(phase-4.2): consumed by cmd::lastwake::run; drop allow then.
#[allow(dead_code)]
pub fn last_kernel_event(
    matcher: &str,
    since: &str,
) -> Result<Option<DateTime<FixedOffset>>, SourceError> {
    let stdout = run_journalctl(since)?;
    let mut last = None;
    for line in stdout.lines() {
        if !line.contains(matcher) {
            continue;
        }
        if let Some(ts) = parse_iso_line(line) {
            last = Some(ts);
        }
    }
    Ok(last)
}

/// Find every journal line containing one of the suspend matchers
/// (`PM: suspend entry` for Sleep, `PM: suspend exit` for Wake) and
/// return them as `SleepEvent`s in chronological order (the order
/// `journalctl` emitted them — oldest first).
///
/// Callers in `-n N` mode use `since = "30 days ago"` to match
/// Python's wider history window — see `powercfg.py` line 1045.
// TODO(phase-4.2): consumed by cmd::lastwake::run; drop allow then.
#[allow(dead_code)]
pub fn list_kernel_events(since: &str) -> Result<Vec<SleepEvent>, SourceError> {
    let stdout = run_journalctl(since)?;
    let mut events = Vec::new();
    for line in stdout.lines() {
        let kind = if line.contains("PM: suspend exit") {
            SleepEventKind::Wake
        } else if line.contains("PM: suspend entry") {
            SleepEventKind::Sleep
        } else {
            continue;
        };
        if let Some(ts) = parse_iso_line(line) {
            events.push(SleepEvent { time: ts, kind });
        }
    }
    Ok(events)
}

/// Extract the leading ISO-8601 timestamp from a journalctl
/// `short-iso` line. The prefix is `YYYY-MM-DDTHH:MM:SS±HH:MM` or
/// `YYYY-MM-DDTHH:MM:SS±HHMM` followed by whitespace and the rest of
/// the line.
///
/// Pub-crate visible so it can be unit-tested directly without round-
/// tripping through `journalctl`.
pub(crate) fn parse_iso_line(line: &str) -> Option<DateTime<FixedOffset>> {
    let token = line.split_whitespace().next()?;
    parse_iso_timestamp(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};
    use std::sync::Mutex;

    // ---- parse_iso_line unit tests ----

    #[test]
    fn parse_iso_line_with_colon_offset() {
        let dt =
            parse_iso_line("2025-12-25T21:40:21-08:00 host kernel: PM: suspend exit").expect("ts");
        assert_eq!(dt.year(), 2025);
        assert_eq!(dt.month(), 12);
        assert_eq!(dt.day(), 25);
        assert_eq!(dt.hour(), 21);
        assert_eq!(dt.minute(), 40);
        assert_eq!(dt.second(), 21);
        assert_eq!(dt.offset().local_minus_utc(), -8 * 3600);
    }

    #[test]
    fn parse_iso_line_with_compact_offset() {
        let dt =
            parse_iso_line("2025-12-25T21:40:21-0800 host kernel: PM: suspend entry").expect("ts");
        assert_eq!(dt.offset().local_minus_utc(), -8 * 3600);
        assert_eq!(dt.hour(), 21);
    }

    #[test]
    fn parse_iso_line_with_no_match() {
        assert!(parse_iso_line("banner not a real journal line").is_none());
    }

    #[test]
    fn parse_iso_line_unparseable_token() {
        // Leading token is non-empty but not an ISO-8601 timestamp.
        assert!(parse_iso_line("abc-def-ghi host kernel:").is_none());
    }

    #[test]
    fn parse_iso_line_empty_input() {
        assert!(parse_iso_line("").is_none());
    }

    // ---- fixture-seam integration tests ----

    /// Tests below mutate `POWERCFG_JOURNAL_FIXTURE`, which is process-
    /// global. Serialize them so they can't race each other.
    static JOURNAL_FIXTURE_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard restoring the prior value of `POWERCFG_JOURNAL_FIXTURE`
    /// on drop (works even if the test panics, unlike a manual
    /// `remove_var` at end-of-test).
    struct JournalFixtureGuard {
        prev: Option<std::ffi::OsString>,
    }

    impl JournalFixtureGuard {
        fn capture() -> Self {
            Self {
                prev: std::env::var_os(JOURNAL_FIXTURE_ENV),
            }
        }
    }

    impl Drop for JournalFixtureGuard {
        fn drop(&mut self) {
            // SAFETY: tests holding a JournalFixtureGuard also hold
            // JOURNAL_FIXTURE_LOCK, so no other thread observes the env
            // while this Drop runs.
            unsafe {
                match self.prev.take() {
                    Some(v) => std::env::set_var(JOURNAL_FIXTURE_ENV, v),
                    None => std::env::remove_var(JOURNAL_FIXTURE_ENV),
                }
            }
        }
    }

    /// Hand-crafted minimal capture mirroring real `journalctl -k -o
    /// short-iso --no-pager` output: a `-- Boot ID --` banner followed
    /// by alternating PM: suspend entry / exit lines.
    fn fixture_path() -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("tests/fixtures/journal-typical.log");
        p
    }

    #[test]
    fn last_kernel_event_returns_most_recent_suspend_exit() {
        let _lock = JOURNAL_FIXTURE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = JournalFixtureGuard::capture();
        // SAFETY: serialized via JOURNAL_FIXTURE_LOCK.
        unsafe { std::env::set_var(JOURNAL_FIXTURE_ENV, fixture_path()) };

        let result =
            last_kernel_event("PM: suspend exit", "ignored").expect("fixture read should succeed");
        let dt = result.expect("fixture has at least one suspend exit");
        // Last `PM: suspend exit` line in the fixture is
        // 2025-04-29T08:22:44-0700.
        assert_eq!(dt.year(), 2025);
        assert_eq!(dt.month(), 4);
        assert_eq!(dt.day(), 29);
        assert_eq!(dt.hour(), 8);
        assert_eq!(dt.minute(), 22);
        assert_eq!(dt.second(), 44);
        assert_eq!(dt.offset().local_minus_utc(), -7 * 3600);
    }

    #[test]
    fn last_kernel_event_returns_most_recent_suspend_entry() {
        let _lock = JOURNAL_FIXTURE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = JournalFixtureGuard::capture();
        // SAFETY: serialized via JOURNAL_FIXTURE_LOCK.
        unsafe { std::env::set_var(JOURNAL_FIXTURE_ENV, fixture_path()) };

        let result =
            last_kernel_event("PM: suspend entry", "ignored").expect("fixture read should succeed");
        let dt = result.expect("fixture has at least one suspend entry");
        // Last `PM: suspend entry` line is 2025-04-29T01:15:18-0700 —
        // notably NOT the same as the last `suspend exit` above.
        assert_eq!(dt.day(), 29);
        assert_eq!(dt.hour(), 1);
        assert_eq!(dt.minute(), 15);
        assert_eq!(dt.second(), 18);
    }

    #[test]
    fn list_kernel_events_returns_all_events_in_order() {
        let _lock = JOURNAL_FIXTURE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = JournalFixtureGuard::capture();
        // SAFETY: serialized via JOURNAL_FIXTURE_LOCK.
        unsafe { std::env::set_var(JOURNAL_FIXTURE_ENV, fixture_path()) };

        let events = list_kernel_events("ignored").expect("fixture read should succeed");
        assert_eq!(events.len(), 6, "fixture has 6 PM: suspend e* lines");
        // Three sleep/wake cycles, alternating Sleep, Wake, Sleep, Wake, ...
        assert_eq!(events[0].kind, SleepEventKind::Sleep);
        assert_eq!(events[1].kind, SleepEventKind::Wake);
        assert_eq!(events[2].kind, SleepEventKind::Sleep);
        assert_eq!(events[3].kind, SleepEventKind::Wake);
        assert_eq!(events[4].kind, SleepEventKind::Sleep);
        assert_eq!(events[5].kind, SleepEventKind::Wake);

        // First Sleep and last Wake bound the captured window.
        assert_eq!(events[0].time.day(), 1);
        assert_eq!(events[0].time.hour(), 3);
        assert_eq!(events[5].time.day(), 29);
        assert_eq!(events[5].time.hour(), 8);
    }

    #[test]
    fn list_kernel_events_skips_unrelated_lines() {
        // The `-- Boot ... --` banner and any other non-PM lines must
        // not produce SleepEvents — guarded by the matcher pre-filter.
        let _lock = JOURNAL_FIXTURE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = JournalFixtureGuard::capture();
        // SAFETY: serialized via JOURNAL_FIXTURE_LOCK.
        unsafe { std::env::set_var(JOURNAL_FIXTURE_ENV, fixture_path()) };

        let events = list_kernel_events("ignored").expect("fixture read should succeed");
        // Fixture file has 7 newline-terminated content lines (1 banner
        // + 6 PM events). Only the 6 PM lines should appear.
        assert_eq!(events.len(), 6);
    }
}
