//! Owning data structures for the `lastwake` subcommand.
//!
//! `cmd::lastwake::run` builds a [`LastWakeReport`] from
//! `source::journal` events plus sysfs/procfs reads (`pm_wakeup_irq`,
//! `/proc/interrupts`) and `dmesg` / ACPI walks for verbose mode.

use chrono::{DateTime, FixedOffset};
use serde::Serialize;

use crate::model::devicequery::AcpiWakeDevice;

/// One sleep or wake transition from the kernel journal.
///
/// Produced by `source::journal::list_kernel_events`. `time` is the
/// leading ISO-8601 timestamp from `journalctl -o short-iso`; `kind`
/// distinguishes `PM: suspend entry` (Sleep) from `PM: suspend exit`
/// (Wake). JSON renames `kind` to `"type"` to match the design schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SleepEvent {
    pub time: DateTime<FixedOffset>,
    #[serde(rename = "type")]
    pub kind: SleepEventKind,
}

/// Whether a `SleepEvent` marks the start (`Sleep`) or end (`Wake`) of
/// a suspend cycle. Serializes lowercase: `"sleep"` / `"wake"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SleepEventKind {
    Sleep,
    Wake,
}

/// Wake IRQ identifier plus its optional `/proc/interrupts` device-name
/// region. Both fields are populated together in `cmd::lastwake::run` —
/// the device name is `None` only when the IRQ isn't listed in
/// `/proc/interrupts` (rare but possible for synthetic IRQs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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
///
/// JSON contract:
/// - `last_sleep`, `last_wake`, `wake_irq` serialize as `null` when
///   absent — distinguishes "data unavailable" from "queryable but
///   empty".
/// - `kernel_messages` (Rust `dmesg_wake`), `acpi_enabled_devices`
///   (Rust `acpi_enabled`), and `history` are omitted from JSON when
///   the corresponding CLI flag is unset (verbose / history). When the
///   flag is set, they serialize as `[]` even if empty so consumers
///   can tell "queried, none found" from "didn't query".
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LastWakeReport {
    /// Most recent `PM: suspend entry` from the 7d window.
    pub last_sleep: Option<DateTime<FixedOffset>>,
    /// Most recent `PM: suspend exit` from the 7d window.
    pub last_wake: Option<DateTime<FixedOffset>>,
    /// `/sys/power/pm_wakeup_irq` + `/proc/interrupts` lookup.
    pub wake_irq: Option<WakeIrq>,
    /// Trailing dmesg lines that mention waking (verbose only).
    /// JSON name: `"kernel_messages"`. Omitted from JSON when None.
    #[serde(rename = "kernel_messages", skip_serializing_if = "Option::is_none")]
    pub dmesg_wake: Option<Vec<String>>,
    /// ACPI wake-capable devices currently enabled (verbose only).
    /// JSON name: `"acpi_enabled_devices"`. Omitted from JSON when None.
    #[serde(
        rename = "acpi_enabled_devices",
        skip_serializing_if = "Option::is_none"
    )]
    pub acpi_enabled: Option<Vec<AcpiWakeDevice>>,
    /// Last `n` sleep/wake events from the 30d window (history mode).
    /// Omitted from JSON when None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<SleepEvent>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    /// Lock the lowercase enum serialization contract.
    #[test]
    fn sleep_event_kind_serializes_lowercase() {
        let s = serde_json::to_string(&SleepEventKind::Sleep).unwrap();
        let w = serde_json::to_string(&SleepEventKind::Wake).unwrap();
        assert_eq!(s, "\"sleep\"");
        assert_eq!(w, "\"wake\"");
    }

    /// All-None report: scalar Option fields render as `null`; the
    /// flag-gated Option<Vec<_>> fields are omitted entirely.
    #[test]
    fn lastwake_report_all_absent_renders_null_or_omits() {
        let report = LastWakeReport::default();
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Scalars: null when None (preserve "data unavailable" signal).
        assert!(v["last_sleep"].is_null());
        assert!(v["last_wake"].is_null());
        assert!(v["wake_irq"].is_null());

        // Flag-gated collections: key is absent.
        assert!(!v.as_object().unwrap().contains_key("kernel_messages"));
        assert!(!v.as_object().unwrap().contains_key("acpi_enabled_devices"),);
        assert!(!v.as_object().unwrap().contains_key("history"));
    }

    /// `null` vs `[]` distinction — the locked contract: a `None`
    /// `Option<Vec<_>>` is omitted from JSON, while a populated
    /// `Option<Vec<_>>` containing an empty Vec serializes as `[]`.
    /// Together with the all-None test above, this proves the two
    /// shapes are distinguishable.
    #[test]
    fn lastwake_report_some_empty_vec_renders_as_array() {
        let report = LastWakeReport {
            history: Some(vec![]),
            dmesg_wake: Some(vec![]),
            acpi_enabled: None,
            ..Default::default()
        };
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Some(empty) → [].
        assert!(v["history"].is_array());
        assert_eq!(v["history"].as_array().unwrap().len(), 0);
        assert!(v["kernel_messages"].is_array());
        // None → omitted.
        assert!(!v.as_object().unwrap().contains_key("acpi_enabled_devices"),);
    }

    /// Populated report: history events render with `type` (renamed
    /// from `kind`) and lowercase enum string.
    #[test]
    fn lastwake_report_history_event_uses_type_and_lowercase() {
        let report = LastWakeReport {
            last_sleep: Some(ts("2025-04-29T01:15:18-07:00")),
            last_wake: Some(ts("2025-04-29T08:22:44-07:00")),
            history: Some(vec![
                SleepEvent {
                    time: ts("2025-04-29T01:15:18-07:00"),
                    kind: SleepEventKind::Sleep,
                },
                SleepEvent {
                    time: ts("2025-04-29T08:22:44-07:00"),
                    kind: SleepEventKind::Wake,
                },
            ]),
            ..Default::default()
        };
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(v["last_sleep"].is_string());
        assert_eq!(v["history"][0]["type"], "sleep");
        assert_eq!(v["history"][1]["type"], "wake");
        // The historical "kind" field name must NOT leak through.
        assert!(v["history"][0].get("kind").is_none());
    }

    /// `wake_irq` populated: serializes as a nested object, never null.
    #[test]
    fn lastwake_report_wake_irq_populated_serializes_object() {
        let report = LastWakeReport {
            wake_irq: Some(WakeIrq {
                irq: "9".into(),
                device: Some("acpi".into()),
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["wake_irq"]["irq"], "9");
        assert_eq!(v["wake_irq"]["device"], "acpi");
    }
}
