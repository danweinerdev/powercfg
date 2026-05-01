//! `waketimers` subcommand — list systemd timer units, partition into
//! wake-capable vs. all timers, and surface the RTC wake alarm.
//!
//! Source-layer errors (D-Bus connection failure, RTC alarm read
//! failure) are swallowed at the call site with `tracing::debug!`,
//! matching the pattern established by sleepstates / devicequery /
//! energy / requests. The printer renders whatever fields are
//! populated — partial data still produces a useful report.

use anyhow::Result;

use crate::cli::Format;
use crate::format::{json, text};
use crate::model::waketimers::{TimerEntry, WakeTimersReport};
use crate::paths::SysRoot;
use crate::source::{dbus, sysfs};

/// Per-call arguments. Mirrors the clap `Command::Waketimers` variant
/// fields plus the `SysRoot` injected by `main` so integration tests
/// can point sysfs readers at fixture trees, and the chosen output
/// [`Format`] (text vs. JSON).
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
    pub root: SysRoot,
    pub format: Format,
}

/// Build a [`WakeTimersReport`] from D-Bus + sysfs and hand it to
/// [`text::print_waketimers`]. Each source's failure is logged at
/// `debug` and swallowed.
pub fn run(args: Args) -> Result<()> {
    let mut report = WakeTimersReport::default();

    // Timers via systemd-manager + per-unit Timer interface. One
    // connection for the whole walk; `list_systemd_timers` borrows it.
    // `all_timers` is only retained in verbose mode so that JSON
    // consumers (Phase 5.2) can distinguish "didn't list" (key
    // omitted) from "listed and found none" (`[]`).
    match dbus::system_bus() {
        Ok(conn) => match dbus::list_systemd_timers(&conn) {
            Ok(timers) => {
                report.wake_timers = wake_capable(&timers);
                if args.verbose {
                    report.all_timers = Some(timers);
                }
            }
            Err(e) => tracing::debug!("list_systemd_timers: {e}"),
        },
        Err(e) => tracing::debug!("system_bus: {e}"),
    }

    match sysfs::read_rtc_wakealarm(&args.root) {
        Ok(v) => report.rtc_wakealarm = v,
        Err(e) => tracing::debug!("read_rtc_wakealarm: {e}"),
    }

    match args.format {
        Format::Text => {
            text::print_waketimers(&report, args.verbose, &mut std::io::stdout().lock())?;
        }
        Format::Json => json::write_report(&report)?,
    }
    Ok(())
}

/// Return the subset of `timers` whose `wake_system` flag is true.
///
/// Extracted as a free function so the partition predicate is unit-
/// testable without a live D-Bus connection — `run` itself can't
/// easily be tested because it opens a real system bus.
fn wake_capable(timers: &[TimerEntry]) -> Vec<TimerEntry> {
    timers.iter().filter(|t| t.wake_system).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_capable_includes_only_wake_system_true() {
        let timers = vec![
            TimerEntry {
                unit: "snapshot.timer".into(),
                wake_system: true,
                next_elapse_realtime_us: 1_000_000,
            },
            TimerEntry {
                unit: "apt-daily.timer".into(),
                wake_system: false,
                next_elapse_realtime_us: 2_000_000,
            },
            TimerEntry {
                unit: "fwupd-refresh.timer".into(),
                wake_system: true,
                next_elapse_realtime_us: 3_000_000,
            },
        ];
        let wake = wake_capable(&timers);
        assert_eq!(wake.len(), 2, "two of three should be wake-capable");
        assert_eq!(wake[0].unit, "snapshot.timer");
        assert_eq!(wake[1].unit, "fwupd-refresh.timer");
    }

    #[test]
    fn wake_capable_empty_input_returns_empty() {
        assert!(wake_capable(&[]).is_empty());
    }

    #[test]
    fn wake_capable_all_false_returns_empty() {
        let timers = vec![TimerEntry {
            unit: "apt-daily.timer".into(),
            wake_system: false,
            next_elapse_realtime_us: 0,
        }];
        assert!(wake_capable(&timers).is_empty());
    }
}
