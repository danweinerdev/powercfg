//! `requests` subcommand — implemented in phase 3.

use anyhow::Result;

/// Stub for the `requests` subcommand.
///
/// Real implementation lands in phase 3 (zbus inhibitor query, audio
/// stream walk, VM detection). Calling this panics with a clear
/// message so accidental dispatches in development surface immediately
/// rather than silently no-op'ing.
pub fn run(_verbose: bool) -> Result<()> {
    unimplemented!("requests: implemented in phase 3")
}
