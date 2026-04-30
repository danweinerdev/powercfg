//! `waketimers` subcommand — implemented in phase 3.

use anyhow::Result;

/// Per-call arguments. Mirrors the clap `Command::Waketimers` variant fields.
// TODO(phase-3): field read by run() when the real handler lands.
#[allow(dead_code)]
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
}

/// Stub for the `waketimers` subcommand.
///
/// Real implementation lands in phase 3 (zbus systemd-manager `ListUnits`
/// plus per-unit `WakeSystem` reads, RTC alarm parsing). Calling this
/// panics with a clear message so accidental dispatches in development
/// surface immediately rather than silently no-op'ing.
pub fn run(_args: Args) -> Result<()> {
    unimplemented!("waketimers: implemented in phase 3")
}
