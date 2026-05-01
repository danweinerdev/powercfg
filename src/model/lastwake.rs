//! Owning data structures for the `lastwake` subcommand.
//!
//! `cmd::lastwake::run` builds a [`LastWakeReport`] from
//! `source::journal` events plus sysfs/procfs reads (`pm_wakeup_irq`,
//! `/proc/interrupts`) and `dmesg` / ACPI walks for verbose mode.

use chrono::{DateTime, FixedOffset};

use crate::model::devicequery::AcpiWakeDevice;

/// One sleep or wake transition from the kernel journal.
///
/// Produced by `source::journal::list_kernel_events`. `time` is the
/// leading ISO-8601 timestamp from `journalctl -o short-iso`; `kind`
/// distinguishes `PM: suspend entry` (Sleep) from `PM: suspend exit`
/// (Wake).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepEvent {
    pub time: DateTime<FixedOffset>,
    pub kind: SleepEventKind,
}

/// Whether a `SleepEvent` marks the start (`Sleep`) or end (`Wake`) of
/// a suspend cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepEventKind {
    Sleep,
    Wake,
}

/// Wake IRQ identifier plus its optional `/proc/interrupts` device-name
/// region. Both fields are populated together in `cmd::lastwake::run` —
/// the device name is `None` only when the IRQ isn't listed in
/// `/proc/interrupts` (rare but possible for synthetic IRQs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeIrq {
    pub irq: String,
    pub device: Option<String>,
}

/// Top-level report consumed by `format::text::print_lastwake`.
///
/// Every field is optional because each source can fail independently
/// — the printer renders whatever is populated and falls back to the
/// "Unknown" sentinel lines for the rest. Mirrors the Python tool's
/// `cmd_lastwake` (powercfg.py 1067-1146): the "system may not have
/// slept this boot" path is reachable when both `journalctl` and
/// `pm_wakeup_irq` come back empty.
#[derive(Debug, Default)]
pub struct LastWakeReport {
    /// Most recent `PM: suspend entry` from the 7d window.
    pub last_sleep: Option<DateTime<FixedOffset>>,
    /// Most recent `PM: suspend exit` from the 7d window.
    pub last_wake: Option<DateTime<FixedOffset>>,
    /// `/sys/power/pm_wakeup_irq` + `/proc/interrupts` lookup.
    pub wake_irq: Option<WakeIrq>,
    /// Trailing dmesg lines that mention waking (verbose only).
    pub dmesg_wake: Option<Vec<String>>,
    /// ACPI wake-capable devices currently enabled (verbose only).
    pub acpi_enabled: Option<Vec<AcpiWakeDevice>>,
    /// Last `n` sleep/wake events from the 30d window (history mode).
    pub history: Option<Vec<SleepEvent>>,
}
