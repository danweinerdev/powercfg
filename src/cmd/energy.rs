//! `energy` subcommand — implemented in phase 2.

use anyhow::Result;

/// Stub for the `energy` subcommand.
///
/// Real implementation lands in phase 2 (power supplies, CPU frequency,
/// hwmon, thermal zones, throttle counters). Calling this panics with a
/// clear message so accidental dispatches in development surface
/// immediately rather than silently no-op'ing.
pub fn run(_verbose: bool) -> Result<()> {
    unimplemented!("energy: implemented in phase 2")
}
