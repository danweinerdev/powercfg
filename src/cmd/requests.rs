//! `requests` subcommand — collect what's preventing sleep on this
//! system from D-Bus (logind inhibitors), procfs (kernel wake locks +
//! VM detection), and pactl (audio streams). Source-layer errors are
//! swallowed at the call site with `tracing::debug!`, matching the
//! pattern established by sleepstates / devicequery / energy.

use anyhow::Result;

use crate::format::text;
use crate::model::requests::RequestsReport;
use crate::paths::SysRoot;
use crate::source::{dbus, procfs, sysfs, userspace};

/// 15-char-truncated `comm` names that the kernel exposes for the
/// VMs we want to detect. `qemu-system-x86_64` truncates to
/// `qemu-system-x86`; `qemu-system-aarch64` to `qemu-system-aar`.
/// Add to this list as new hypervisors become relevant.
const VM_COMM_NAMES: &[&str] = &[
    "qemu",
    "qemu-system-x86", // truncation of qemu-system-x86_64
    "qemu-system-aar", // truncation of qemu-system-aarch64
    "VBoxHeadless",
];

/// Per-call arguments. Mirrors the clap `Command::Requests` variant
/// fields plus the `SysRoot` injected by `main` so integration tests
/// can point procfs/sysfs readers at fixture trees.
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
    pub root: SysRoot,
}

/// Build a [`RequestsReport`] from the four data sources and hand it
/// to [`text::print_requests`]. Each source's failure is logged at
/// `debug` and swallowed — the printer renders whatever fields are
/// populated, matching the Python tool's tolerance for partial data.
pub fn run(args: Args) -> Result<()> {
    let mut report = RequestsReport::default();

    // Inhibitors from logind via D-Bus. Open one connection for the
    // whole run; `list_inhibitors` borrows it.
    match dbus::system_bus() {
        Ok(conn) => match dbus::list_inhibitors(&conn) {
            Ok(v) => report.inhibitors = v,
            Err(e) => tracing::debug!("list_inhibitors: {e}"),
        },
        Err(e) => tracing::debug!("system_bus: {e}"),
    }

    match sysfs::read_kernel_wake_locks(&args.root) {
        Ok(v) => report.wake_locks = v,
        Err(e) => tracing::debug!("read_kernel_wake_locks: {e}"),
    }

    match userspace::list_audio_streams() {
        Ok(v) => report.audio_streams = v,
        Err(e) => tracing::debug!("list_audio_streams: {e}"),
    }

    match procfs::find_processes_by_comm(&args.root, VM_COMM_NAMES) {
        Ok(v) => report.vms = v,
        Err(e) => tracing::debug!("find_processes_by_comm: {e}"),
    }

    if args.verbose {
        match sysfs::read_usb_wakeup_devices(&args.root) {
            Ok(v) => report.usb_wakeup = v,
            Err(e) => tracing::debug!("read_usb_wakeup_devices: {e}"),
        }
    }

    text::print_requests(&report, args.verbose, &mut std::io::stdout().lock())?;
    Ok(())
}
