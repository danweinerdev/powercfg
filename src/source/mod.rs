//! Data acquisition layer.
//!
//! Each submodule reads from one transport (sysfs, procfs, D-Bus, journal,
//! subprocess) and returns plain model structs. Source functions return
//! `Result<T, SourceError>`; the command layer applies
//! `.unwrap_or_default()` / `.ok()` per call site to choose presentation
//! behavior. See `Designs/RustRewrite/README.md` Decision 7.

pub mod error;

// Re-exported so command modules can write `use crate::source::SourceError`.
// Currently unused at the crate root because the 1.2 utilities are not yet
// wired into a command handler; the re-export earns its keep starting at
// task 1.4 when `cmd::sleepstates` consumes the source layer.
#[allow(unused_imports)]
pub use error::SourceError;
