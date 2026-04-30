//! Text-mode printers for each subcommand's report struct.
//!
//! Output must match the Python tool's printed format byte-for-byte where
//! reasonable; integration tests assert this via `insta` snapshots. New
//! printers land here per-subcommand as Phase 1+ implements each handler.

use crate::model::sleepstates::SleepStatesReport;

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
    println!();
    println!("[MEMORY SLEEP MODE]");
    println!("{}", "-".repeat(30));
    if let Some(current) = &report.mem_current {
        let desc = lookup_mem_mode(current).unwrap_or(current.as_str());
        println!("  Current: {current} - {desc}");
    }
    if !report.mem_modes.is_empty() {
        println!("  Available: {}", report.mem_modes.join(", "));
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
