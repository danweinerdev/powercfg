//! Text-mode printers for each subcommand's report struct.
//!
//! Output must match the Python tool's printed format byte-for-byte where
//! reasonable; integration tests assert this via `insta` snapshots. New
//! printers land here per-subcommand as Phase 1+ implements each handler.

use crate::format::freq::format_freq;
use crate::model::devicequery::DeviceQueryReport;
use crate::model::energy::EnergyReport;
use crate::model::sleepstates::SleepStatesReport;
use crate::paths::SysRoot;
use crate::source::sysfs;

/// Sleep-state-token → (long name, ACPI label, verbose description).
///
/// Mirrors the dict in `cmd_sleepstates` (`powercfg.py` lines 562-567).
/// Tokens not in this table render with just the bare token (no
/// description), matching Python's `if state in state_desc:` guard.
const STATE_DESC: &[(&str, &str, &str, &str)] = &[
    (
        "freeze",
        "Suspend-to-Idle",
        "S0ix",
        "Lowest latency, moderate power savings",
    ),
    (
        "mem",
        "Suspend-to-RAM",
        "S3",
        "Fast resume, good power savings",
    ),
    (
        "disk",
        "Hibernation",
        "S4",
        "Slowest resume, best power savings",
    ),
    ("standby", "Standby", "S1", "Light sleep, minimal savings"),
];

/// Memory-sleep-mode token → human description.
///
/// Mirrors `cmd_sleepstates` lines 570-574. Lookups for unknown modes
/// fall back to the bare token (Python's `mem_mode_desc.get(current, current)`).
const MEM_MODE_DESC: &[(&str, &str)] = &[
    ("s2idle", "Suspend-to-Idle (software-driven)"),
    ("shallow", "Shallow suspend (platform-assisted)"),
    ("deep", "Suspend-to-RAM (hardware S3)"),
];

fn lookup_state(
    token: &str,
) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    STATE_DESC.iter().find(|row| row.0 == token)
}

fn lookup_mem_mode(token: &str) -> Option<&'static str> {
    MEM_MODE_DESC
        .iter()
        .find(|(name, _)| *name == token)
        .map(|(_, desc)| *desc)
}

/// Print a [`SleepStatesReport`] in the Python tool's text format.
///
/// Verbose mode adds a description line under each known state, lists
/// available disk modes, and appends a `[HIBERNATION IMAGE]` section when
/// `/sys/power/image_size` was readable.
pub fn print_sleepstates(report: &SleepStatesReport, verbose: bool) {
    println!("AVAILABLE SLEEP STATES");
    println!("{}", "=".repeat(50));

    // [SLEEP STATES]
    println!();
    println!("[SLEEP STATES]");
    println!("{}", "-".repeat(30));
    if report.states.is_empty() {
        println!("  Unable to read sleep states");
    } else {
        for state in &report.states {
            match lookup_state(state) {
                Some((_, name, acpi, desc)) => {
                    println!("  {state:<10} {name} ({acpi})");
                    if verbose {
                        println!("             {desc}");
                    }
                }
                None => println!("  {state}"),
            }
        }
    }

    // [MEMORY SLEEP MODE]
    //
    // Diverges from Python parity: the Python tool prints the section header
    // even when /sys/power/mem_sleep is unreadable (no fallback message),
    // mirrored from its [SLEEP STATES] fallback inconsistently. Adding the
    // fallback message here makes the failure mode legible without changing
    // the happy-path output.
    println!();
    println!("[MEMORY SLEEP MODE]");
    println!("{}", "-".repeat(30));
    if report.mem_current.is_none() && report.mem_modes.is_empty() {
        println!("  Unable to read memory sleep mode");
    } else {
        if let Some(current) = &report.mem_current {
            let desc = lookup_mem_mode(current).unwrap_or(current.as_str());
            println!("  Current: {current} - {desc}");
        }
        if !report.mem_modes.is_empty() {
            println!("  Available: {}", report.mem_modes.join(", "));
        }
    }

    // [HIBERNATION MODE] — only when "disk" is in the supported states.
    // Matches Python's `if 'disk' in states:` guard at line 604.
    if report.states.iter().any(|s| s == "disk") {
        println!();
        println!("[HIBERNATION MODE]");
        println!("{}", "-".repeat(30));
        if let Some(current) = &report.disk_current {
            println!("  Current: {current}");
        }
        if !report.disk_modes.is_empty() && verbose {
            println!("  Available: {}", report.disk_modes.join(", "));
        }

        // Swap viability — Python prints this nested inside the
        // hibernation section.
        if report.swaps.is_empty() {
            println!("  Swap: None configured (hibernation unavailable)");
        } else {
            let total_swap_mb: u64 = report.swaps.iter().map(|s| s.size_kb).sum::<u64>() / 1024;
            println!("  Swap: {total_swap_mb} MB available");
            for swap in &report.swaps {
                let size_gb = swap.size_kb as f64 / 1024.0 / 1024.0;
                let note = if swap.device.contains("zram") {
                    " (may not support hibernation)"
                } else {
                    ""
                };
                println!("    {}: {:.1} GB{}", swap.device, size_gb, note);
            }
        }
    }

    // Verbose: image size limit.
    if verbose {
        if let Some(bytes) = report.image_size_bytes {
            let image_size_mb = bytes / (1024 * 1024);
            println!();
            println!("[HIBERNATION IMAGE]");
            println!("{}", "-".repeat(30));
            println!("  Max image size: {image_size_mb} MB");
        }
    }

    println!();
    println!("{}", "=".repeat(50));
}

/// Print a [`DeviceQueryReport`] in the Python tool's text format.
///
/// `enabled_only` is purely a display filter on which ACPI rows render
/// in the table. The summary footer always walks the full
/// `report.acpi_devices` list, so the X-of-Y ratio is identical in
/// either mode (only the rows-shown-above-the-summary change). Matches
/// Python's behavior: in Python the `enabled_only` filter happens to
/// also reduce `devices` so the same final count falls out, but the
/// Rust code reaches that result by always walking the unfiltered list.
///
/// `verbose` adds a `Wake count: N` line under each ACPI row whose PCI
/// `wakeup_count` is non-zero. Stats failures and zero counts are
/// silently skipped.
///
/// The `[USB WAKE DEVICES]` section is omitted entirely when no USB
/// wake devices exist (matching Python `if usb_devices:`), so headless
/// hosts get a tighter output.
pub fn print_devicequery(
    report: &DeviceQueryReport,
    root: &SysRoot,
    verbose: bool,
    enabled_only: bool,
) {
    println!("WAKE-CAPABLE DEVICES");
    println!("{}", "=".repeat(50));

    println!();
    println!("[ACPI WAKE DEVICES]");
    println!("{}", "-".repeat(30));

    let acpi: Vec<&_> = report
        .acpi_devices
        .iter()
        .filter(|d| !enabled_only || d.enabled)
        .collect();

    if acpi.is_empty() {
        println!("  No ACPI wake devices found.");
    } else {
        // Python: f"  {'Device':<8} {'State':<6} {'Status':<10} {'Description'}"
        println!(
            "  {:<8} {:<6} {:<10} Description",
            "Device", "State", "Status",
        );
        // Underline row matching Python's per-column dash widths.
        println!(
            "  {:<8} {:<6} {:<10} {}",
            "-".repeat(6),
            "-".repeat(5),
            "-".repeat(8),
            "-".repeat(20),
        );

        for dev in &acpi {
            let status = if dev.enabled { "enabled" } else { "disabled" };
            let desc = match &dev.sysfs {
                None => String::new(),
                Some(s) => match sysfs::read_pci_device_description(root, s) {
                    Ok(Some(d)) => d,
                    // NotFound, other Err, or Ok(None): fall back to the
                    // raw sysfs token (Python line 456: `desc = dev["sysfs"]`).
                    _ => s.clone(),
                },
            };
            println!(
                "  {:<8} {:<6} {:<10} {}",
                dev.device, dev.state, status, desc,
            );

            if verbose {
                if let Some(s) = &dev.sysfs {
                    if let Ok(stats) = sysfs::read_pci_wakeup_stats(root, s) {
                        if stats.wakeup_count > 0 {
                            // Python line 465: 11-space indent before
                            // "Wake count:".
                            println!("           Wake count: {}", stats.wakeup_count);
                        }
                    }
                }
            }
        }
    }

    if !report.usb_devices.is_empty() {
        println!();
        println!("[USB WAKE DEVICES]");
        println!("{}", "-".repeat(30));
        for dev in &report.usb_devices {
            // Status is always "enabled" for USB entries — the walker
            // only includes wake-enabled devices.
            println!("  {:<12} {:<10} {}", dev.device, "enabled", dev.name);
        }
    }

    // Summary uses the unfiltered counts. When `enabled_only` is true
    // the ACPI list is already filtered to enabled rows, so the count
    // collapses to the same number; when false, this is the count of
    // enabled devices in the full list. Either way we compute against
    // `report.acpi_devices` directly.
    let enabled_acpi = report.acpi_devices.iter().filter(|d| d.enabled).count();
    let enabled_count = enabled_acpi + report.usb_devices.len();
    let total_count = report.acpi_devices.len() + report.usb_devices.len();
    println!();
    println!("{}", "=".repeat(50));
    println!("Wake-enabled devices: {enabled_count} of {total_count}");
}

/// Print an [`EnergyReport`] in the Python tool's text format.
///
/// Section structure (matches `cmd_energy`, powercfg.py 940-1012):
/// - `[POWER SUPPLIES]` — always prints. Empty supplies list renders the
///   `No power supplies detected (desktop system)` fallback (Python 967).
/// - `[CPU FREQUENCY]` — always prints. Each per-field line is
///   conditional on the matching `Option` being `Some`. The current
///   frequency line additionally collapses to bare-current when min/max
///   aren't both present, and is suppressed entirely when `cur_freq_khz`
///   is `None`. `verbose` adds a 4-space-indented `Available:` line under
///   `Energy preference:` listing the kernel-supplied EPP options.
/// - `[TEMPERATURES]` — suppressed when `report.temperatures` is empty
///   (Python's `if temps:` guard at line 994).
/// - `[THERMAL THROTTLING]` — suppressed when `throttle_count == 0`. The
///   Python tool's `Status: THROTTLED`/`Status: Not throttled` line is
///   intentionally absent (the Python flag was based on a misread of the
///   thermal-zone `mode` ABI; dropped at the 2.3 quality pass).
pub fn print_energy(report: &EnergyReport, verbose: bool) {
    println!("ENERGY STATUS");
    println!("{}", "=".repeat(50));

    // [POWER SUPPLIES]
    println!();
    println!("[POWER SUPPLIES]");
    println!("{}", "-".repeat(30));
    if report.supplies.is_empty() {
        println!("  No power supplies detected (desktop system)");
    } else {
        for ps in &report.supplies {
            let mut desc = match &ps.status {
                Some(s) => s.clone(),
                None => "Unknown".to_owned(),
            };
            if let Some(pct) = ps.capacity_pct {
                desc.push_str(&format!(" ({pct}%)"));
            } else if let Some(level) = &ps.level {
                desc.push_str(&format!(" ({level})"));
            }
            if let Some(uw) = ps.power_uw {
                let watts = uw as f64 / 1_000_000.0;
                desc.push_str(&format!(" - {watts:.1}W"));
            }
            println!("  {}: {desc}", ps.name);
        }
    }

    // [CPU FREQUENCY]
    println!();
    println!("[CPU FREQUENCY]");
    println!("{}", "-".repeat(30));
    if let Some(cpu) = &report.cpu {
        if let Some(driver) = &cpu.driver {
            println!("  Driver: {driver}");
        }
        if let Some(governor) = &cpu.governor {
            println!("  Governor: {governor}");
        }
        if let Some(cur) = cpu.cur_freq_khz {
            let cur_s = format_freq(cur);
            match (cpu.min_freq_khz, cpu.max_freq_khz) {
                (Some(min), Some(max)) => {
                    let min_s = format_freq(min);
                    let max_s = format_freq(max);
                    println!("  Current: {cur_s} (range: {min_s} - {max_s})");
                }
                _ => {
                    println!("  Current: {cur_s}");
                }
            }
        }
        if let Some(epp) = &cpu.epp {
            println!("  Energy preference: {epp}");
            if verbose {
                if let Some(avail) = &cpu.epp_available {
                    println!("    Available: {avail}");
                }
            }
        }
        // Python line 991 always prints CPU cores from cpu_info; mirror
        // that here even when cpu_count is zero (which can happen if
        // /sys/devices/system/cpu was unreadable but cpufreq fields
        // somehow populated).
        println!("  CPU cores: {}", cpu.cpu_count);
    }

    // [TEMPERATURES]
    if !report.temperatures.is_empty() {
        println!();
        println!("[TEMPERATURES]");
        println!("{}", "-".repeat(30));
        for t in &report.temperatures {
            println!("  {}: {:.1}°C", t.label, t.temp_c);
        }
    }

    // [THERMAL THROTTLING]
    if report.throttle.throttle_count > 0 {
        println!();
        println!("[THERMAL THROTTLING]");
        println!("{}", "-".repeat(30));
        println!(
            "  Historical throttle events: {}",
            report.throttle.throttle_count
        );
    }

    println!();
    println!("{}", "=".repeat(50));
}
