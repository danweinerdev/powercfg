//! `devicequery` subcommand — collect ACPI + USB wake-capable devices and
//! print the table. Source-layer errors are swallowed at the call site
//! with debug logging, matching the pattern established by sleepstates.

use anyhow::Result;

use crate::cli::Format;
use crate::format::{json, text};
use crate::model::devicequery::DeviceQueryReport;
use crate::paths::SysRoot;
use crate::source::{procfs, sysfs};

/// Per-call arguments. Mirrors the clap `Command::Devicequery` variant
/// fields plus the `SysRoot` resolved by `main` and the chosen output
/// [`Format`] (text vs. JSON).
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
    pub enabled_only: bool,
    pub root: SysRoot,
    pub format: Format,
}

/// Build a [`DeviceQueryReport`] by reading procfs (ACPI wakeup) and
/// sysfs (USB devices) under `args.root`, then print it via
/// [`text::print_devicequery`]. The printer also reaches back into
/// sysfs for per-device PCI descriptors and wakeup-stat counters in
/// verbose mode, which is why it takes a `&SysRoot`.
pub fn run(args: Args) -> Result<()> {
    let mut report = DeviceQueryReport::default();

    match procfs::read_acpi_wakeup(&args.root) {
        Ok(devices) => report.acpi_devices = devices,
        Err(e) => tracing::debug!("read_acpi_wakeup: {e}"),
    }

    match sysfs::read_usb_wakeup_devices(&args.root) {
        Ok(devices) => report.usb_devices = devices,
        Err(e) => tracing::debug!("read_usb_wakeup_devices: {e}"),
    }

    match args.format {
        Format::Text => {
            // The text printer reaches back into sysfs (PCI descriptors,
            // wakeup-stat counters) — the JSON branch deliberately skips
            // that enrichment because the JSON contract is shape-only.
            text::print_devicequery(&report, &args.root, args.verbose, args.enabled_only);
        }
        Format::Json => json::write_report(&report)?,
    }
    Ok(())
}
