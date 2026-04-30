//! `requests` subcommand — implemented in phase 3.

use anyhow::Result;

/// Per-call arguments. Mirrors the clap `Command::Requests` variant fields so
/// future args (e.g. a `Format` reference once Phase 5 lands) extend this
/// struct rather than the `run` signature.
// TODO(phase-3): field read by run() when the real handler lands.
#[allow(dead_code)]
#[derive(Debug)]
pub struct Args {
    pub verbose: bool,
}

/// Stub for the `requests` subcommand.
///
/// Real implementation lands in phase 3 (zbus inhibitor query, audio
/// stream walk, VM detection). Calling this panics with a clear
/// message so accidental dispatches in development surface immediately
/// rather than silently no-op'ing.
pub fn run(_args: Args) -> Result<()> {
    unimplemented!("requests: implemented in phase 3")
}
