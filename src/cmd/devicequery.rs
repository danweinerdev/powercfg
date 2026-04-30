//! `devicequery` subcommand — implemented in phase 2.

use anyhow::Result;

/// Per-call arguments. Mirrors the clap `Command::Devicequery` variant fields.
// TODO(phase-2): fields read by run() when the real handler lands.
#[allow(dead_code)]
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
    pub enabled_only: bool,
}

/// Stub for the `devicequery` subcommand.
///
/// Real implementation lands in phase 2 (ACPI wakeup parsing, USB/PCI
/// device walking, vendor/class lookup). Calling this panics with a
/// clear message so accidental dispatches in development surface
/// immediately rather than silently no-op'ing.
pub fn run(_args: Args) -> Result<()> {
    unimplemented!("devicequery: implemented in phase 2")
}
