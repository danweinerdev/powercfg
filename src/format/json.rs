//! JSON output helper shared across command modules.
//!
//! `cmd::*::run` calls [`write_report`] when `args.format` is
//! [`crate::cli::Format::Json`]. The helper serializes the report
//! through a single `stdout().lock()` guard and emits a trailing
//! newline through the same writer — this avoids re-locking stdout
//! (which would happen if we called `println!()` after the write) and
//! keeps the JSON object terminated by a `\n` so consumers like `jq`
//! can trail it cleanly.
//!
//! `serde_json::to_writer_pretty` does not emit a final newline; the
//! caller is responsible for it. We wrap the serializer error in
//! `anyhow::Context` so a serialization failure surfaces a useful
//! message — in practice the report structs only carry types that
//! `serde_json` can always serialize, so this branch is defensive.

use std::io::Write;

use anyhow::{Context, Result};
use serde::Serialize;

/// Serialize `report` to stdout as pretty-printed JSON, terminated by a
/// trailing newline.
pub fn write_report<T: Serialize>(report: &T) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, report)
        .context("failed to serialize report as JSON")?;
    // Same lock guard, explicit `\n` — avoids the double-lock that
    // `println!()` would introduce after dropping `handle`.
    handle
        .write_all(b"\n")
        .context("failed to write trailing newline to stdout")?;
    Ok(())
}
