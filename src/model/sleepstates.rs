//! Report struct for the `sleepstates` subcommand.

use crate::source::procfs::SwapDevice;

/// Owning data for the `sleepstates` subcommand.
///
/// Built by `cmd::sleepstates`, consumed by
/// `format::text::print_sleepstates`. Phase 5 adds
/// `#[derive(Serialize)]` for `--json` output; bare for now.
#[derive(Debug, Default)]
pub struct SleepStatesReport {
    /// Tokens from `/sys/power/state` (`["freeze", "mem", "disk"]`).
    pub states: Vec<String>,
    /// Modes from `/sys/power/mem_sleep`, brackets stripped.
    pub mem_modes: Vec<String>,
    /// Currently selected mem_sleep mode (the `[bracketed]` token), if any.
    pub mem_current: Option<String>,
    /// Modes from `/sys/power/disk`, brackets stripped.
    pub disk_modes: Vec<String>,
    /// Currently selected disk mode (the `[bracketed]` token), if any.
    pub disk_current: Option<String>,
    /// Swap areas from `/proc/swaps`.
    pub swaps: Vec<SwapDevice>,
    /// Maximum hibernation image size in bytes from `/sys/power/image_size`.
    /// `None` if the file is absent or unreadable.
    pub image_size_bytes: Option<u64>,
}
