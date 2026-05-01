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

use std::io::{ErrorKind, Write};

use anyhow::Result;
use serde::Serialize;

/// Serialize `report` to stdout as pretty-printed JSON, terminated by a
/// trailing newline.
///
/// `BrokenPipe` is treated as a clean exit: a downstream consumer like
/// `head` closing the pipe is the conventional Unix signal to stop —
/// surfacing it as `error: ...` would be a regression versus the text
/// path, where `println!` already swallows write errors.
pub fn write_report<T: Serialize>(report: &T) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    if let Err(e) = serde_json::to_writer_pretty(&mut handle, report) {
        if e.io_error_kind() == Some(ErrorKind::BrokenPipe) {
            return Ok(());
        }
        return Err(anyhow::Error::new(e).context("failed to serialize report as JSON"));
    }
    // Same lock guard, explicit `\n` — avoids the double-lock that
    // `println!()` would introduce after dropping `handle`.
    if let Err(e) = handle.write_all(b"\n") {
        if e.kind() == ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(anyhow::Error::new(e).context("failed to write trailing newline to stdout"));
    }
    Ok(())
}
