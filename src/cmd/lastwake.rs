//! `lastwake` subcommand — implemented in phase 4.

use anyhow::Result;

/// Per-call arguments. Mirrors the clap `Command::Lastwake` variant fields.
// TODO(phase-4): fields read by run() when the real handler lands.
#[allow(dead_code)]
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
    pub history: Option<usize>,
}

/// Stub for the `lastwake` subcommand.
///
/// Real implementation lands in phase 4 (`journalctl` subprocess +
/// in-process sleep/wake event parsing, history pagination). Calling
/// this panics with a clear message so accidental dispatches in
/// development surface immediately rather than silently no-op'ing.
pub fn run(_args: Args) -> Result<()> {
    unimplemented!("lastwake: implemented in phase 4")
}
