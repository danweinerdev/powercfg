//! `lastwake` subcommand — collect last sleep/wake timestamps from the
//! kernel journal, the wake-source IRQ from sysfs+procfs, and (in
//! verbose / history modes) dmesg + ACPI device + journal-history data.
//!
//! Source-layer errors are swallowed at the call site with `match` +
//! `tracing::debug!`, matching the pattern established by sleepstates /
//! devicequery / energy / requests / waketimers. The printer renders
//! whatever fields are populated — partial data still produces a useful
//! report (the `Unknown` / `Not available` fallbacks are reachable
//! whenever the corresponding source returned `None` or errored).

use anyhow::Result;

use crate::format::text;
use crate::model::lastwake::{LastWakeReport, WakeIrq};
use crate::paths::SysRoot;
use crate::source::{journal, procfs, sysfs, userspace};

/// Default journal window for the most-recent sleep/wake lookup. Matches
/// Python `lastwake` (powercfg.py 247, 269).
const DEFAULT_SINCE: &str = "7 days ago";

/// Wider journal window for the `-n N` history mode. Matches Python
/// `get_sleep_history` (powercfg.py 1045).
const HISTORY_SINCE: &str = "30 days ago";

/// Per-call arguments. Mirrors the clap `Command::Lastwake` variant
/// fields plus the `SysRoot` injected by `main` so integration tests
/// can point sysfs/procfs readers at fixture trees.
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
    pub history: Option<usize>,
    pub root: SysRoot,
}

/// Build a [`LastWakeReport`] from the journal + sysfs/procfs sources
/// and hand it to [`text::print_lastwake`]. Each source's failure is
/// logged at `debug` and swallowed — the report is best-effort.
pub fn run(args: Args) -> Result<()> {
    let mut report = LastWakeReport::default();

    // Most recent sleep/wake from the 7-day window.
    match journal::last_kernel_event("PM: suspend entry", DEFAULT_SINCE) {
        Ok(t) => report.last_sleep = t,
        Err(e) => tracing::debug!("last_kernel_event(suspend entry): {e}"),
    }
    match journal::last_kernel_event("PM: suspend exit", DEFAULT_SINCE) {
        Ok(t) => report.last_wake = t,
        Err(e) => tracing::debug!("last_kernel_event(suspend exit): {e}"),
    }

    // Wake IRQ + device-name lookup. Both reads are best-effort; either
    // one failing leaves the corresponding field absent.
    match sysfs::read_wake_irq(&args.root) {
        Ok(Some(irq)) => {
            let device = match procfs::read_irq_info(&args.root, &irq) {
                Ok(info) => info,
                Err(e) => {
                    tracing::debug!("read_irq_info: {e}");
                    None
                }
            };
            report.wake_irq = Some(WakeIrq { irq, device });
        }
        Ok(None) => {}
        Err(e) => tracing::debug!("read_wake_irq: {e}"),
    }

    if args.verbose {
        match userspace::dmesg_wake_lines() {
            Ok(lines) => report.dmesg_wake = Some(lines),
            Err(e) => tracing::debug!("dmesg_wake_lines: {e}"),
        }
        match procfs::read_acpi_wakeup(&args.root) {
            Ok(devs) => {
                let enabled = devs.into_iter().filter(|d| d.enabled).collect();
                report.acpi_enabled = Some(enabled);
            }
            Err(e) => tracing::debug!("read_acpi_wakeup: {e}"),
        }
    }

    if let Some(n) = args.history {
        match journal::list_kernel_events(HISTORY_SINCE) {
            Ok(events) => {
                let take_from = events.len().saturating_sub(n);
                report.history = Some(events[take_from..].to_vec());
            }
            Err(e) => tracing::debug!("list_kernel_events: {e}"),
        }
    }

    text::print_lastwake(
        &report,
        args.verbose,
        args.history,
        &mut std::io::stdout().lock(),
    )?;
    Ok(())
}
