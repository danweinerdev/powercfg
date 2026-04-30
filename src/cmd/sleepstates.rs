//! `sleepstates` subcommand — collect sleep-state data from sysfs/procfs
//! and hand the populated [`SleepStatesReport`] to the text formatter.
//!
//! Each source call site swallows its error with a `debug!` log so the
//! presentation matches Python's `try/except: pass` while keeping the
//! failure observable under `RUST_LOG=debug`. Per `Designs/RustRewrite`
//! Decision 7, the swallow happens at the call site, not in the source
//! layer.

use anyhow::Result;

use crate::format::text;
use crate::model::sleepstates::SleepStatesReport;
use crate::paths::SysRoot;
use crate::source::{procfs, sysfs};

/// Per-call arguments. Mirrors the clap `Command::Sleepstates` variant
/// fields plus the `SysRoot` resolved by `main`.
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
    pub root: SysRoot,
}

/// Build a [`SleepStatesReport`] by reading sysfs/procfs under
/// `args.root`, then print it via [`text::print_sleepstates`].
pub fn run(args: Args) -> Result<()> {
    let mut report = SleepStatesReport::default();

    match sysfs::read_sleep_states(&args.root) {
        Ok(states) => report.states = states,
        Err(e) => tracing::debug!("read_sleep_states: {e}"),
    }

    match sysfs::read_mem_sleep_modes(&args.root) {
        Ok((modes, current)) => {
            report.mem_modes = modes;
            report.mem_current = current;
        }
        Err(e) => tracing::debug!("read_mem_sleep_modes: {e}"),
    }

    match sysfs::read_disk_modes(&args.root) {
        Ok((modes, current)) => {
            report.disk_modes = modes;
            report.disk_current = current;
        }
        Err(e) => tracing::debug!("read_disk_modes: {e}"),
    }

    match procfs::read_swaps(&args.root) {
        Ok(swaps) => report.swaps = swaps,
        Err(e) => tracing::debug!("read_swaps: {e}"),
    }

    match sysfs::read_image_size_bytes(&args.root) {
        Ok(bytes) => report.image_size_bytes = Some(bytes),
        Err(e) => tracing::debug!("read_image_size_bytes: {e}"),
    }

    text::print_sleepstates(&report, args.verbose);
    Ok(())
}
