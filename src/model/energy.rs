//! Owning data structures for the `energy` subcommand.
//!
//! All five types are populated by `cmd::energy::run` from the
//! `source::sysfs` readers added alongside this module.

use serde::Serialize;

/// One row from `/sys/class/power_supply/<name>/`.
///
/// `kind`, `status`, `capacity_pct`, `level`, and `power_uw` are each
/// independently `Option<…>` because individual files may be absent on a
/// given supply (an AC adapter has no `capacity`; some batteries don't
/// expose `power_now`). `name` is always present — it's the directory
/// name the walker found.
///
/// JSON renames `kind` to `"type"` to match the design schema. Optional
/// fields serialize as `null` when absent (preserve unavailability).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PowerSupply {
    pub name: String, // "BAT0", "AC", etc.
    #[serde(rename = "type")]
    pub kind: Option<String>, // "Battery", "Mains", ...
    pub status: Option<String>, // "Discharging", "Full", ...
    pub capacity_pct: Option<u8>, // 0..=100
    pub level: Option<String>, // "Normal", "Low", ...
    pub power_uw: Option<u64>, // microwatts; None when power_now absent
}

/// Snapshot of `/sys/devices/system/cpu/cpu0/cpufreq/`.
///
/// Read from cpu0 only (matching the Python tool); the assumption is that
/// driver/governor/freq settings are uniform across cores. `cpu_count` is
/// the number of `cpuN` directories (regex-free `cpu` + all-digits suffix
/// match) under `/sys/devices/system/cpu`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CpuFreqInfo {
    pub driver: Option<String>,   // "amd-pstate-epp", "intel_pstate", ...
    pub governor: Option<String>, // "powersave", "performance", ...
    pub cur_freq_khz: Option<u64>,
    pub min_freq_khz: Option<u64>,
    pub max_freq_khz: Option<u64>,
    pub epp: Option<String>,           // "balance_performance", ...
    pub epp_available: Option<String>, // raw space-separated list from sysfs
    pub cpu_count: usize,
}

/// One `tempN_input` reading under `/sys/class/hwmon/<chip>/`, paired
/// with its `tempN_label` (or the literal `"CPU"` fallback when no label
/// file is present). Mirrors the Python tool's narrow filter to the
/// `k10temp`/`coretemp`/`zenpower` chips — other hwmon entries (NVMe,
/// fans, GPUs) are skipped so the section stays CPU-thermals only.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ThermalReading {
    pub label: String,  // "Tctl", "Tdie", or "CPU" fallback
    pub temp_c: f64,    // converted from millidegrees
    pub source: String, // hwmon "name" file value: "k10temp", "coretemp", ...
}

/// Historical CPU thermal-throttle counter — sum of
/// `cpu*/thermal_throttle/package_throttle_count` across all CPUs since
/// boot. Non-zero values are an "this machine has been thermally
/// limited at least N times" signal; the printer renders them as a
/// historical-events line.
///
/// Diverges from Python: the Python tool also exposed a `throttled`
/// boolean derived from `thermal_zone*/mode == "disabled"`, but that is
/// not what the kernel ABI means — `mode = "disabled"` indicates a
/// thermal zone has been administratively turned OFF (e.g., by a
/// userspace thermal daemon taking over), not that the CPU is currently
/// being throttled. On 99%+ of machines the Python flag prints
/// `Not throttled` regardless of actual thermal state, so the field is
/// dropped here and the printer relies on `throttle_count > 0` as the
/// only meaningful signal. The JSON schema sketch in the design doc
/// still shows `throttled`; the actual JSON omits it (documented in
/// `docs/json-schema.md`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ThrottleStatus {
    pub throttle_count: u64, // sum of cpu*/thermal_throttle/package_throttle_count
}

/// Top-level report consumed by `format::text::print_energy`.
///
/// `cpu` is non-optional because `read_cpu_freq_info` returns
/// `Ok(CpuFreqInfo::default())` rather than `Err` when the cpufreq dir
/// is absent — a default `CpuFreqInfo` already represents "nothing
/// known about cpufreq" with all `Option`s `None` and `cpu_count` 0.
/// Wrapping it in another `Option` would just be a defensive layer
/// that never fires.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct EnergyReport {
    pub supplies: Vec<PowerSupply>,
    pub cpu: CpuFreqInfo,
    pub temperatures: Vec<ThermalReading>,
    pub throttle: ThrottleStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default report: collections render as `[]`, never omitted; the
    /// nested `cpu` and `throttle` objects are present with their
    /// default values.
    #[test]
    fn energy_report_default_renders_expected_shape() {
        let report = EnergyReport::default();
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(v["supplies"].is_array());
        assert_eq!(v["supplies"].as_array().unwrap().len(), 0);
        assert!(v["temperatures"].is_array());
        assert_eq!(v["temperatures"].as_array().unwrap().len(), 0);
        // Nested objects always present.
        assert!(v["cpu"].is_object());
        assert!(v["throttle"].is_object());
        assert_eq!(v["throttle"]["throttle_count"], 0);
        // The dropped `throttled` field is NOT in the output.
        assert!(v["throttle"].get("throttled").is_none());
    }

    /// Populated report: `kind` field is renamed to `"type"`; CPU
    /// snake_case keys match the schema.
    #[test]
    fn energy_report_populated_renames_supply_kind_to_type() {
        let report = EnergyReport {
            supplies: vec![PowerSupply {
                name: "BAT0".into(),
                kind: Some("Battery".into()),
                status: Some("Discharging".into()),
                capacity_pct: Some(87),
                level: Some("Normal".into()),
                power_uw: Some(12_500_000),
            }],
            cpu: CpuFreqInfo {
                driver: Some("amd-pstate-epp".into()),
                governor: Some("powersave".into()),
                cur_freq_khz: Some(3_400_000),
                min_freq_khz: Some(400_000),
                max_freq_khz: Some(4_800_000),
                epp: Some("balance_performance".into()),
                epp_available: Some("performance balance_performance balance_power power".into()),
                cpu_count: 16,
            },
            temperatures: vec![ThermalReading {
                label: "Tctl".into(),
                temp_c: 52.4,
                source: "k10temp".into(),
            }],
            throttle: ThrottleStatus { throttle_count: 0 },
        };
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        // `kind` → `"type"` rename.
        assert_eq!(v["supplies"][0]["type"], "Battery");
        assert!(v["supplies"][0].get("kind").is_none());
        assert_eq!(v["supplies"][0]["name"], "BAT0");
        assert_eq!(v["supplies"][0]["capacity_pct"], 87);
        assert_eq!(v["supplies"][0]["power_uw"], 12_500_000_u64);

        assert_eq!(v["cpu"]["driver"], "amd-pstate-epp");
        assert_eq!(v["cpu"]["cur_freq_khz"], 3_400_000_u64);
        assert_eq!(v["cpu"]["cpu_count"], 16);

        assert_eq!(v["temperatures"][0]["label"], "Tctl");
        assert_eq!(v["temperatures"][0]["source"], "k10temp");
    }

    /// Supply with all optional fields None: each renders as null,
    /// not omitted (preserves per-field unavailability).
    #[test]
    fn energy_report_supply_options_render_null_when_absent() {
        let report = EnergyReport {
            supplies: vec![PowerSupply {
                name: "AC".into(),
                kind: None,
                status: None,
                capacity_pct: None,
                level: None,
                power_uw: None,
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let supply = &v["supplies"][0];
        assert_eq!(supply["name"], "AC");
        assert!(supply["type"].is_null());
        assert!(supply["status"].is_null());
        assert!(supply["capacity_pct"].is_null());
        assert!(supply["level"].is_null());
        assert!(supply["power_uw"].is_null());
    }
}
