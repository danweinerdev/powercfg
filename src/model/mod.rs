//! Owning report structs returned by command handlers.
//!
//! Each subcommand builds one `<Cmd>Report` from source data and hands it
//! to the formatter. Phase 5 adds `#[derive(Serialize)]` so the same
//! structs feed `--json` output; until then they're plain owning data.

pub mod devicequery;
pub mod energy;
pub mod lastwake;
pub mod requests;
pub mod sleepstates;
pub mod waketimers;
