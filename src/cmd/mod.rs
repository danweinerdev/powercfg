//! Subcommand handlers.
//!
//! Each module owns one `powercfg <name>` subcommand and exposes a
//! `run(...)` function that returns `anyhow::Result<()>`. The dispatcher
//! in `main.rs` matches on the parsed `cli::Command` enum and forwards
//! the relevant flags. Per the phase 1 plan, only `sleepstates` is
//! implemented (as a graceful no-op stub at 1.3, real body at 1.4); the
//! rest panic via `unimplemented!()` pinned to their delivery phase so
//! accidental dispatches surface immediately.

pub mod devicequery;
pub mod energy;
pub mod lastwake;
pub mod requests;
pub mod sleepstates;
pub mod waketimers;
