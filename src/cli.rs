//! clap derive structs for the `powercfg` CLI surface.
//!
//! The shape mirrors the original Python tool's `argparse` configuration
//! verbatim: six subcommands (`requests`, `lastwake`, `devicequery`,
//! `sleepstates`, `waketimers`, `energy`) with their per-command flags
//! (`-v`/`--verbose` everywhere, `-n`/`--history` on `lastwake`,
//! `--enabled-only` on `devicequery`). The global `--json` flag selects
//! the [`Format`] each command's `run` branches on after building its
//! report.

use clap::{Parser, Subcommand, ValueEnum};

/// Output format for command reports. `Text` is the default and matches
/// the Python tool's printed output verbatim; `Json` serializes the
/// underlying report struct via `serde_json` (see `docs/json-schema.md`
/// for the per-command shape).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Format {
    #[default]
    Text,
    Json,
}

/// Top-level CLI parser.
#[derive(Parser, Debug)]
#[command(
    name = "powercfg",
    version,
    about = "Linux power configuration utility (similar to Windows powercfg)"
)]
pub struct Cli {
    /// Emit the report as JSON instead of formatted text.
    ///
    /// See `docs/json-schema.md` for the per-command output shape.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Resolve the chosen output format from parsed flags.
    pub fn format(&self) -> Format {
        if self.json {
            Format::Json
        } else {
            Format::Text
        }
    }
}

/// The six top-level subcommands. Each variant carries exactly the flags
/// declared by the matching `argparse` block in `powercfg.py`.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Display power requests (what's preventing sleep)
    Requests {
        /// Show additional information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Display information about the last wake event
    Lastwake {
        /// Show additional information (ACPI devices, kernel messages)
        #[arg(short, long)]
        verbose: bool,

        /// Show last N sleep/wake events
        #[arg(short = 'n', long = "history", value_name = "N")]
        history: Option<usize>,
    },

    /// Display devices that can wake the system
    Devicequery {
        /// Show wakeup statistics for devices
        #[arg(short, long)]
        verbose: bool,

        /// Only show devices with wakeup enabled
        #[arg(long = "enabled-only")]
        enabled_only: bool,
    },

    /// Display available sleep states and configuration
    Sleepstates {
        /// Show additional details
        #[arg(short, long)]
        verbose: bool,
    },

    /// Display scheduled wake timers
    Waketimers {
        /// Show all scheduled timers, not just wake-capable ones
        #[arg(short, long)]
        verbose: bool,
    },

    /// Display energy and power status
    Energy {
        /// Show additional details
        #[arg(short, long)]
        verbose: bool,
    },
}
