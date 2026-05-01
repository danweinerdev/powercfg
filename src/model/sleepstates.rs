//! Report struct for the `sleepstates` subcommand.

use serde::Serialize;

use crate::source::procfs::SwapDevice;

/// Owning data for the `sleepstates` subcommand.
///
/// Built by `cmd::sleepstates`, consumed by
/// `format::text::print_sleepstates`.
///
/// JSON contract:
/// - The schema is intentionally flat (does not nest `mem_sleep` or
///   `hibernation` sub-objects) — see `docs/json-schema.md` for the
///   deviation note. The data is the same; the structure differs
///   from the design doc sketch.
/// - `mem_current`, `disk_current` serialize as `null` when absent
///   (preserve "unavailable" signal).
/// - `image_size_bytes` is omitted when `None` because population is
///   gated on `--verbose` in `cmd::sleepstates::run`. JSON consumers
///   that see the key know `--verbose` was set.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
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
    /// `None` if the file is absent or unreadable, OR if `--verbose` was
    /// not set (`cmd::sleepstates::run` only populates it in verbose mode
    /// so the JSON contract "image size only with -v" is satisfied).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_size_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All-default report: `mem_current`, `disk_current` render as
    /// `null`; `image_size_bytes` is OMITTED (verbose-gated).
    #[test]
    fn sleepstates_report_default_renders_expected_shape() {
        let report = SleepStatesReport::default();
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Vec keys always present as arrays, never omitted.
        assert!(v["states"].is_array());
        assert!(v["mem_modes"].is_array());
        assert!(v["disk_modes"].is_array());
        assert!(v["swaps"].is_array());
        // Scalar Options: null when None.
        assert!(v["mem_current"].is_null());
        assert!(v["disk_current"].is_null());
        // image_size_bytes: omitted when None.
        assert!(!v.as_object().unwrap().contains_key("image_size_bytes"));
    }

    /// Populated report: image_size_bytes appears when Some; swap
    /// rows render with `kind` renamed to `"type"`.
    #[test]
    fn sleepstates_report_populated_renders_image_and_swap_type() {
        let report = SleepStatesReport {
            states: vec!["freeze".into(), "mem".into(), "disk".into()],
            mem_modes: vec!["s2idle".into(), "deep".into()],
            mem_current: Some("deep".into()),
            disk_modes: vec!["platform".into(), "shutdown".into()],
            disk_current: Some("platform".into()),
            swaps: vec![SwapDevice {
                device: "/dev/dm-0".into(),
                kind: "partition".into(),
                size_kb: 16_384_000,
            }],
            image_size_bytes: Some(4_294_967_296),
        };
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(v["mem_current"], "deep");
        assert_eq!(v["disk_current"], "platform");
        assert_eq!(v["image_size_bytes"], 4_294_967_296_u64);
        // SwapDevice's `kind` field renames to `"type"` per the schema.
        assert_eq!(v["swaps"][0]["type"], "partition");
        assert!(v["swaps"][0].get("kind").is_none());
        assert_eq!(v["swaps"][0]["device"], "/dev/dm-0");
        assert_eq!(v["swaps"][0]["size_kb"], 16_384_000_u64);
    }
}
