//! Owning data structures for the `energy` subcommand.
//!
//! Phase 5 adds `#[derive(Serialize)]`; left bare for now to mirror the
//! pattern in `model::sleepstates` and `model::devicequery`.
//!
//! All five types are populated by `cmd::energy::run` from the
//! `source::sysfs` readers added alongside this module.

/// One row from `/sys/class/power_supply/<name>/`.
///
/// `kind`, `status`, `capacity_pct`, `level`, and `power_uw` are each
/// independently `Option<…>` because individual files may be absent on a
/// given supply (an AC adapter has no `capacity`; some batteries don't
/// expose `power_now`). `name` is always present — it's the directory
/// name the walker found.
#[derive(Debug, Clone)]
pub struct PowerSupply {
    pub name: String, // "BAT0", "AC", etc.
    // TODO(phase-5): consumed by JSON output; text printer doesn't use it.
    #[allow(dead_code)]
    pub kind: Option<String>, // "Battery", "Mains", ...
    pub status: Option<String>,   // "Discharging", "Full", ...
    pub capacity_pct: Option<u8>, // 0..=100
    pub level: Option<String>,    // "Normal", "Low", ...
    pub power_uw: Option<u64>,    // microwatts; None when power_now absent
}

/// Snapshot of `/sys/devices/system/cpu/cpu0/cpufreq/`.
///
/// Read from cpu0 only (matching the Python tool); the assumption is that
/// driver/governor/freq settings are uniform across cores. `cpu_count` is
/// the number of `cpuN` directories (regex-free `cpu` + all-digits suffix
/// match) under `/sys/devices/system/cpu`.
#[derive(Debug, Clone, Default)]
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
#[derive(Debug, Clone)]
pub struct ThermalReading {
    pub label: String, // "Tctl", "Tdie", or "CPU" fallback
    pub temp_c: f64,   // converted from millidegrees
    // TODO(phase-5): consumed by JSON output; text printer doesn't use it.
    #[allow(dead_code)]
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
/// only meaningful signal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThrottleStatus {
    pub throttle_count: u64, // sum of cpu*/thermal_throttle/package_throttle_count
}

/// Top-level report consumed by `format::text::print_energy`.
#[derive(Debug, Default)]
pub struct EnergyReport {
    pub supplies: Vec<PowerSupply>,
    pub cpu: Option<CpuFreqInfo>, // None if cpufreq unavailable (rare)
    pub temperatures: Vec<ThermalReading>,
    pub throttle: ThrottleStatus,
}
