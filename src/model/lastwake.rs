//! Owning data structures for the `lastwake` subcommand.
//!
//! `cmd::lastwake::run` (4.2) builds a `LastWakeReport` from
//! `source::journal` events plus sysfs reads (`pm_wakeup_irq`).
//! 4.1 lands the journal data plumbing only; the higher-level report
//! struct (and the wake-IRQ reader that feeds it) follow in 4.2.

use chrono::{DateTime, FixedOffset};

/// One sleep or wake transition from the kernel journal.
///
/// Produced by `source::journal::list_kernel_events`. `time` is the
/// leading ISO-8601 timestamp from `journalctl -o short-iso`; `kind`
/// distinguishes `PM: suspend entry` (Sleep) from `PM: suspend exit`
/// (Wake).
// TODO(phase-4.2): wired by cmd::lastwake::run; drop allow then.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepEvent {
    pub time: DateTime<FixedOffset>,
    pub kind: SleepEventKind,
}

/// Whether a `SleepEvent` marks the start (`Sleep`) or end (`Wake`) of
/// a suspend cycle.
// TODO(phase-4.2): wired by cmd::lastwake::run; drop allow then.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepEventKind {
    Sleep,
    Wake,
}

// LastWakeReport, WakeIrq are added by 4.2 — keep this module
// minimal so 4.1 ships without speculative shape.
