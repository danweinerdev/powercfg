//! `sleepstates` subcommand — collect sleep-state data from sysfs/procfs
//! and hand the populated [`SleepStatesReport`] to the text formatter.
//!
//! Each source call site swallows its error with a `debug!` log so the
//! presentation matches Python's `try/except: pass` while keeping the
//! failure observable under `RUST_LOG=debug`. Per `Designs/RustRewrite`
//! Decision 7, the swallow happens at the call site, not in the source
//! layer.

use anyhow::Result;

use crate::cli::Format;
use crate::format::{json, text};
use crate::model::sleepstates::SleepStatesReport;
use crate::paths::SysRoot;
use crate::source::{procfs, sysfs};

/// Per-call arguments. Mirrors the clap `Command::Sleepstates` variant
/// fields plus the `SysRoot` resolved by `main` and the chosen output
/// [`Format`] (text vs. JSON).
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
    pub root: SysRoot,
    pub format: Format,
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

    // `image_size_bytes` is gated on `--verbose` because the JSON
    // contract requires the key only appear with `-v`. The text
    // printer also already wraps its render in `if verbose`, so
    // this gating doesn't change text output.
    if args.verbose {
        match sysfs::read_image_size_bytes(&args.root) {
            Ok(bytes) => report.image_size_bytes = Some(bytes),
            Err(e) => tracing::debug!("read_image_size_bytes: {e}"),
        }
    }

    match args.format {
        Format::Text => text::print_sleepstates(&report, args.verbose),
        Format::Json => json::write_report(&report)?,
    }
    Ok(())
}
