//! `waketimers` subcommand — list systemd timer units, partition into
//! wake-capable vs. all timers, and surface the RTC wake alarm.
//!
//! Source-layer errors (D-Bus connection failure, RTC alarm read
//! failure) are swallowed at the call site with `tracing::debug!`,
//! matching the pattern established by sleepstates / devicequery /
//! energy / requests. The printer renders whatever fields are
//! populated — partial data still produces a useful report.

use anyhow::Result;

use crate::format::text;
use crate::model::waketimers::WakeTimersReport;
use crate::paths::SysRoot;
use crate::source::{dbus, sysfs};

/// Per-call arguments. Mirrors the clap `Command::Waketimers` variant
/// fields plus the `SysRoot` injected by `main` so integration tests
/// can point sysfs readers at fixture trees.
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
    pub root: SysRoot,
}

/// Build a [`WakeTimersReport`] from D-Bus + sysfs and hand it to
/// [`text::print_waketimers`]. Each source's failure is logged at
/// `debug` and swallowed.
pub fn run(args: Args) -> Result<()> {
    let mut report = WakeTimersReport::default();

    // Timers via systemd-manager + per-unit Timer interface. One
    // connection for the whole walk; `list_systemd_timers` borrows it.
    match dbus::system_bus() {
        Ok(conn) => match dbus::list_systemd_timers(&conn) {
            Ok(timers) => {
                report.wake_timers = timers.iter().filter(|t| t.wake_system).cloned().collect();
                report.all_timers = timers;
            }
            Err(e) => tracing::debug!("list_systemd_timers: {e}"),
        },
        Err(e) => tracing::debug!("system_bus: {e}"),
    }

    match sysfs::read_rtc_wakealarm(&args.root) {
        Ok(v) => report.rtc_wakealarm = v,
        Err(e) => tracing::debug!("read_rtc_wakealarm: {e}"),
    }

    text::print_waketimers(&report, args.verbose, &mut std::io::stdout().lock())?;
    Ok(())
}
