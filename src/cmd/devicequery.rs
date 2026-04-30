//! `devicequery` subcommand — implemented in phase 2.

use anyhow::Result;

/// Stub for the `devicequery` subcommand.
///
/// Real implementation lands in phase 2 (ACPI wakeup parsing, USB/PCI
/// device walking, vendor/class lookup). Calling this panics with a
/// clear message so accidental dispatches in development surface
/// immediately rather than silently no-op'ing.
pub fn run(_verbose: bool, _enabled_only: bool) -> Result<()> {
    unimplemented!("devicequery: implemented in phase 2")
}
