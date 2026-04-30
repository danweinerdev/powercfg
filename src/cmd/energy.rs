//! `energy` subcommand — implemented in phase 2.

use anyhow::Result;

/// Per-call arguments. Mirrors the clap `Command::Energy` variant fields.
// TODO(phase-2): field read by run() when the real handler lands.
#[allow(dead_code)]
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
}

/// Stub for the `energy` subcommand.
///
/// Real implementation lands in phase 2 (power supplies, CPU frequency,
/// hwmon, thermal zones, throttle counters). Calling this panics with a
/// clear message so accidental dispatches in development surface
/// immediately rather than silently no-op'ing.
pub fn run(_args: Args) -> Result<()> {
    unimplemented!("energy: implemented in phase 2")
}
