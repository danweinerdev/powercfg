//! `lastwake` subcommand — implemented in phase 4.

use anyhow::Result;

/// Stub for the `lastwake` subcommand.
///
/// Real implementation lands in phase 4 (`journalctl` subprocess +
/// in-process sleep/wake event parsing, history pagination). Calling
/// this panics with a clear message so accidental dispatches in
/// development surface immediately rather than silently no-op'ing.
pub fn run(_verbose: bool, _history: Option<usize>) -> Result<()> {
    unimplemented!("lastwake: implemented in phase 4")
}
