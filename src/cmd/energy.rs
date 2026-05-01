//! `energy` subcommand — collect power supply, CPU frequency, thermal,
//! and throttle data; hand to the text printer. Errors swallowed at
//! the call site with debug logging, matching the established pattern.

use anyhow::Result;

use crate::cli::Format;
use crate::format::{json, text};
use crate::model::energy::EnergyReport;
use crate::paths::SysRoot;
use crate::source::sysfs;

/// Per-call arguments. Mirrors the clap `Command::Energy` variant fields
/// plus the `SysRoot` injected by `main` so integration tests can point
/// the readers at fixture trees via `POWERCFG_SYSROOT`, and the chosen
/// output [`Format`] (text vs. JSON).
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
    pub root: SysRoot,
    pub format: Format,
}

/// Build an [`EnergyReport`] from the four sysfs readers and hand it
/// to [`text::print_energy`]. Each reader's failure is logged at
/// `debug` and swallowed — the printer renders whatever fields are
/// populated, matching the Python tool's tolerance for partial data.
pub fn run(args: Args) -> Result<()> {
    let mut report = EnergyReport::default();

    match sysfs::read_power_supplies(&args.root) {
        Ok(s) => report.supplies = s,
        Err(e) => tracing::debug!("read_power_supplies: {e}"),
    }

    match sysfs::read_cpu_freq_info(&args.root) {
        Ok(c) => report.cpu = c,
        Err(e) => tracing::debug!("read_cpu_freq_info: {e}"),
    }

    match sysfs::read_thermal_info(&args.root) {
        Ok(t) => report.temperatures = t,
        Err(e) => tracing::debug!("read_thermal_info: {e}"),
    }

    match sysfs::read_throttle_status(&args.root) {
        Ok(t) => report.throttle = t,
        Err(e) => tracing::debug!("read_throttle_status: {e}"),
    }

    match args.format {
        Format::Text => text::print_energy(&report, args.verbose),
        Format::Json => json::write_report(&report)?,
    }
    Ok(())
}
