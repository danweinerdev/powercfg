//! `waketimers` subcommand — implemented in phase 3.

use anyhow::Result;

/// Stub for the `waketimers` subcommand.
///
/// Real implementation lands in phase 3 (zbus systemd-manager `ListUnits`
/// plus per-unit `WakeSystem` reads, RTC alarm parsing). Calling this
/// panics with a clear message so accidental dispatches in development
/// surface immediately rather than silently no-op'ing.
pub fn run(_verbose: bool) -> Result<()> {
    unimplemented!("waketimers: implemented in phase 3")
}
