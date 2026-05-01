//! Text-mode printers for each subcommand's report struct.
//!
//! Output must match the Python tool's printed format byte-for-byte where
//! reasonable; integration tests assert this via `insta` snapshots. New
//! printers land here per-subcommand as Phase 1+ implements each handler.

use std::io::Write;
use std::time::Duration;

use crate::format::duration::format_duration;
use crate::format::freq::format_freq;
use crate::model::devicequery::DeviceQueryReport;
use crate::model::energy::EnergyReport;
use crate::model::lastwake::{LastWakeReport, SleepEventKind};
use crate::model::requests::{AudioStream, RequestsReport};
use crate::model::sleepstates::SleepStatesReport;
use crate::model::waketimers::WakeTimersReport;
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
    let cpu = &report.cpu;
    println!();
    println!("[CPU FREQUENCY]");
    println!("{}", "-".repeat(30));
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

/// Render an `AudioStream` into the per-stream `Client:` line value.
///
/// Eight cases over (name, binary, pid). The most common live cases
/// are "all three present" (browsers, music players) and "all three
/// missing" (system streams that don't expose process metadata, e.g.
/// the screen-recording mixer). The intermediate combinations are
/// rarer but legitimate — `application.name` without
/// `application.process.id` happens for sandboxed PipeWire streams,
/// for instance — so the printer handles each one explicitly rather
/// than collapsing to a fallback.
fn format_audio_client(stream: &AudioStream) -> String {
    match (
        stream.application_name.as_deref(),
        stream.binary.as_deref(),
        stream.pid,
    ) {
        (Some(name), Some(binary), Some(pid)) => format!("{name} ({binary}, PID: {pid})"),
        (Some(name), None, Some(pid)) => format!("{name} (PID: {pid})"),
        (Some(name), Some(binary), None) => format!("{name} ({binary})"),
        (Some(name), None, None) => name.to_string(),
        (None, Some(binary), Some(pid)) => format!("{binary} (PID: {pid})"),
        (None, Some(binary), None) => binary.to_string(),
        (None, None, Some(pid)) => format!("PID {pid}"),
        (None, None, None) => "<unknown>".to_string(),
    }
}

/// Map an inhibitor's resolved `comm` string to the human-readable VM
/// label the printer renders. `comm` values come from
/// `source::procfs::find_processes_by_comm` so they're already trimmed
/// and 15-char-truncated.
fn vm_label(comm: &str) -> &str {
    if comm.starts_with("qemu") {
        "QEMU/KVM VM"
    } else if comm == "VBoxHeadless" {
        "VirtualBox VM"
    } else {
        comm
    }
}

/// Print a [`RequestsReport`] in the Python tool's text format
/// (`cmd_requests`, powercfg.py 1163-1232).
///
/// Takes a `&mut dyn Write` rather than calling `println!` so unit
/// tests can capture and snapshot the output without spawning the
/// binary or going through OS-level stdout redirection. The rest of
/// the printers in this module use `println!` because their
/// integration tests use `assert_cmd` against the binary; the
/// `requests` report is built in-process from D-Bus + procfs +
/// userspace data, none of which can be cleanly faked through the
/// binary's environment, so it gets a writer parameter instead.
///
/// Section ordering and behavior:
/// - `[SYSTEM INHIBITORS]`: iterates ALL inhibitors but prints only
///   those whose `what` field contains `sleep` or `idle`
///   (case-insensitive). Empty list (no inhibitors at all) renders
///   `None.`. The summary footer counts the unfiltered list.
/// - `[KERNEL WAKE LOCKS]`: one per line; empty → `None.`.
/// - `[AUDIO STREAMS]`: per-stream id+client; empty → `None.`.
/// - `[VIRTUAL MACHINES]`: maps `comm` to a friendly label
///   (`qemu*` → `QEMU/KVM VM`, `VBoxHeadless` → `VirtualBox VM`,
///   otherwise raw); empty → `None.`.
/// - `[USB WAKEUP DEVICES]`: only if `verbose=true`; not counted in
///   the summary. Empty → `None.`.
/// - Trailing summary: `Total sleep blockers found: N` (sum of the
///   unfiltered inhibitor count + wake_locks + audio_streams + vms,
///   matches Python line 1227) or `No active sleep blockers detected.`
///   when total is zero.
pub fn print_requests(
    report: &RequestsReport,
    verbose: bool,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(out, "POWER REQUEST STATUS")?;
    writeln!(out, "{}", "=".repeat(50))?;

    // [SYSTEM INHIBITORS]
    writeln!(out)?;
    writeln!(out, "[SYSTEM INHIBITORS]")?;
    writeln!(out, "{}", "-".repeat(30))?;
    if report.inhibitors.is_empty() {
        writeln!(out, "  None.")?;
    } else {
        // Display filter: only sleep/idle inhibitors render. Other
        // entries are skipped here but counted in the summary footer
        // (matches Python lines 1175 + 1227). The fallback `None.`
        // line above only fires when the input list is empty — a
        // non-empty list whose entries all fail this filter still
        // renders nothing under the header (matches Python).
        for inh in &report.inhibitors {
            let blocks = inh.what.to_lowercase();
            if !(blocks.contains("sleep") || blocks.contains("idle")) {
                continue;
            }
            writeln!(out, "  Process: {} (PID: {})", inh.comm, inh.pid)?;
            // Diverges from Python: `systemd-inhibit --list` resolves
            // numeric uid to a username via NSS; logind's D-Bus
            // ListInhibitors API only exposes uid as a u32. Resolving
            // via getpwuid_r adds a libc dependency for one column in
            // a status-only command; the cost-benefit isn't worth it.
            // Render the numeric uid instead.
            writeln!(out, "    User: {}", inh.uid)?;
            writeln!(out, "    Blocks: {}", inh.what)?;
            writeln!(out, "    Reason: {}", inh.why)?;
            writeln!(out)?;
        }
    }

    // [KERNEL WAKE LOCKS]
    writeln!(out)?;
    writeln!(out, "[KERNEL WAKE LOCKS]")?;
    writeln!(out, "{}", "-".repeat(30))?;
    if report.wake_locks.is_empty() {
        writeln!(out, "  None.")?;
    } else {
        for lock in &report.wake_locks {
            writeln!(out, "  {lock}")?;
        }
    }

    // [AUDIO STREAMS]
    writeln!(out)?;
    writeln!(out, "[AUDIO STREAMS]")?;
    writeln!(out, "{}", "-".repeat(30))?;
    if report.audio_streams.is_empty() {
        writeln!(out, "  None.")?;
    } else {
        for stream in &report.audio_streams {
            writeln!(out, "  Stream ID: {}", stream.id)?;
            let client = format_audio_client(stream);
            writeln!(out, "    Client: {client}")?;
        }
    }

    // [VIRTUAL MACHINES]
    writeln!(out)?;
    writeln!(out, "[VIRTUAL MACHINES]")?;
    writeln!(out, "{}", "-".repeat(30))?;
    if report.vms.is_empty() {
        writeln!(out, "  None.")?;
    } else {
        for vm in &report.vms {
            writeln!(out, "  {} (PID: {})", vm_label(&vm.comm), vm.pid)?;
        }
    }

    // [USB WAKEUP DEVICES] — verbose only, not counted in summary.
    if verbose {
        writeln!(out)?;
        writeln!(out, "[USB WAKEUP DEVICES]")?;
        writeln!(out, "{}", "-".repeat(30))?;
        if report.usb_wakeup.is_empty() {
            writeln!(out, "  None.")?;
        } else {
            for dev in &report.usb_wakeup {
                writeln!(out, "  {} ({})", dev.name, dev.device)?;
            }
        }
    }

    // Trailing summary. Counts the UNFILTERED inhibitor list so a
    // shutdown/handle-power-key inhibitor that doesn't display still
    // shows up in the total — matches Python line 1227's
    // `len(inhibitors)` against the same unfiltered list.
    writeln!(out)?;
    writeln!(out, "{}", "=".repeat(50))?;
    let total = report.inhibitors.len()
        + report.wake_locks.len()
        + report.audio_streams.len()
        + report.vms.len();
    if total > 0 {
        writeln!(out, "Total sleep blockers found: {total}")?;
    } else {
        writeln!(out, "No active sleep blockers detected.")?;
    }
    Ok(())
}

/// Maximum rows in the verbose `[ALL SCHEDULED TIMERS]` table before
/// the `... and N more timers` truncation suffix kicks in. Matches the
/// Python tool's `all_timers[:15]` slice (powercfg.py line 745).
const ALL_TIMERS_DISPLAY_CAP: usize = 15;

/// Print a [`WakeTimersReport`] in the Python tool's text format
/// (`cmd_waketimers`, powercfg.py 710-767).
///
/// Takes a `&mut dyn Write` so unit tests can capture the output and
/// snapshot it without going through `assert_cmd`. The printer is
/// total — empty sections still render their headers + a fallback
/// line, matching Python.
///
/// Section ordering and behavior:
/// - Header: `WAKE TIMERS` + `=` × 50.
/// - `[TIMERS WITH WAKESYSTEM=YES]` (always): one entry per
///   `wake_timers`, formatted as `  <unit>` then
///   `    Next: <next_elapse_display>`. Empty list →
///   `  None - no timers will wake the system from sleep`.
/// - `[ALL SCHEDULED TIMERS]` (verbose only, only if `all_timers`
///   non-empty): table with `Timer` (≤33 chars), `Wakes` (Yes/No),
///   `Next` (≤25 chars) columns. Capped at 15 rows; overflow renders
///   `  ... and N more timers`.
/// - `[RTC WAKE ALARM]` (always): `  Scheduled wake: <s>` when set,
///   `  No RTC wake alarm set` otherwise.
/// - Trailing summary: `Wake timers active: N` when non-empty,
///   `No active wake timers` when zero.
pub fn print_waketimers(
    report: &WakeTimersReport,
    verbose: bool,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(out, "WAKE TIMERS")?;
    writeln!(out, "{}", "=".repeat(50))?;

    // [TIMERS WITH WAKESYSTEM=YES]
    writeln!(out)?;
    writeln!(out, "[TIMERS WITH WAKESYSTEM=YES]")?;
    writeln!(out, "{}", "-".repeat(30))?;
    if report.wake_timers.is_empty() {
        writeln!(out, "  None - no timers will wake the system from sleep")?;
    } else {
        for timer in &report.wake_timers {
            writeln!(out, "  {}", timer.unit)?;
            writeln!(out, "    Next: {}", timer.next_elapse_display())?;
        }
    }

    // [ALL SCHEDULED TIMERS] — verbose only, only if non-empty.
    // Matches Python's `if args.verbose and all_timers:` guard
    // (powercfg.py line 740): an empty list under verbose still
    // skips the section.
    if verbose && !report.all_timers.is_empty() {
        writeln!(out)?;
        writeln!(out, "[ALL SCHEDULED TIMERS]")?;
        writeln!(out, "{}", "-".repeat(30))?;
        // Header + underline. Widths match Python lines 743-744:
        // Timer column padded to 35 (max name 33 + 2 spaces),
        // Wakes column padded to 6, Next column unpadded.
        writeln!(out, "  {:<35} {:<6} Next", "Timer", "Wakes")?;
        writeln!(
            out,
            "  {:<35} {:<6} {}",
            "-".repeat(33),
            "-".repeat(4),
            "-".repeat(20),
        )?;
        for timer in report.all_timers.iter().take(ALL_TIMERS_DISPLAY_CAP) {
            // Truncate names > 33 chars (Python: `timer["unit"][:33]`).
            // chars().take preserves UTF-8 boundaries, but `.timer`
            // unit names are ASCII so byte-slicing would also work.
            let unit: String = timer.unit.chars().take(33).collect();
            let wakes = if timer.wake_system { "Yes" } else { "No" };
            let next_full = timer.next_elapse_display();
            // Python: `next[:25] if len(next) > 25 else next` — chars
            // again to avoid splitting a UTF-8 codepoint.
            let next: String = next_full.chars().take(25).collect();
            writeln!(out, "  {unit:<35} {wakes:<6} {next}")?;
        }
        if report.all_timers.len() > ALL_TIMERS_DISPLAY_CAP {
            let extra = report.all_timers.len() - ALL_TIMERS_DISPLAY_CAP;
            writeln!(out, "  ... and {extra} more timers")?;
        }
    }

    // [RTC WAKE ALARM]
    writeln!(out)?;
    writeln!(out, "[RTC WAKE ALARM]")?;
    writeln!(out, "{}", "-".repeat(30))?;
    match &report.rtc_wakealarm {
        Some(s) => writeln!(out, "  Scheduled wake: {s}")?,
        None => writeln!(out, "  No RTC wake alarm set")?,
    }

    // Trailing summary.
    writeln!(out)?;
    writeln!(out, "{}", "=".repeat(50))?;
    if report.wake_timers.is_empty() {
        writeln!(out, "No active wake timers")?;
    } else {
        writeln!(out, "Wake timers active: {}", report.wake_timers.len())?;
    }
    Ok(())
}

/// Maximum width of a kernel-wake-message line in the verbose output
/// before the truncation suffix kicks in. Matches Python's `if len(line)
/// > 70: line = line[:67] + "..."` (powercfg.py 1118-1119).
const KERNEL_WAKE_LINE_MAX: usize = 70;

/// Render a `DateTime<FixedOffset>` in the same shape `journalctl -o
/// short-iso` produced (e.g. `2025-04-29T08:22:44-0700`). Python keeps
/// the raw matched string; the Rust caller has a parsed `DateTime` so
/// we re-format with the equivalent specifier.
fn format_journal_ts(ts: chrono::DateTime<chrono::FixedOffset>) -> String {
    ts.format("%Y-%m-%dT%H:%M:%S%z").to_string()
}

/// Print a [`LastWakeReport`] in the Python tool's text format
/// (`cmd_lastwake`, powercfg.py 1067-1146).
///
/// Section ordering and behavior:
/// - Header: `LAST WAKE INFORMATION` + `=` × 50.
/// - `[LAST SLEEP/WAKE CYCLE]`: prints `Sleep time:` and `Wake time:`
///   lines, falling back to `Unknown` (sleep) or
///   `Unknown (system may not have slept this boot)` (wake) when the
///   journal source returned `None`. A `Duration:` line follows when
///   both timestamps are present and `wake - sleep > 0`.
/// - `[WAKE SOURCE]`: prints `Wake IRQ: <irq>` and (when non-`None`)
///   `Device: <info>`. Falls back to `Wake IRQ: Not available` when no
///   IRQ was recorded.
/// - `[KERNEL WAKE MESSAGES]` (verbose only, only if the dmesg list is
///   non-empty): prints up to 5 lines truncated to 70 chars (Python's
///   `line[:67] + "..."` rule).
/// - `[ENABLED ACPI WAKE DEVICES]` (verbose only): prints each enabled
///   ACPI device as `<device>: <state> (<sysfs>)` (or without the
///   parenthesized sysfs when absent). Empty list → `  None.`.
/// - `[RECENT SLEEP/WAKE HISTORY]` (history mode only): prints up to
///   `history_count` events as `  <ts> - <KIND>` (uppercase).
/// - Trailing `=` × 50.
pub fn print_lastwake(
    report: &LastWakeReport,
    verbose: bool,
    history_count: Option<usize>,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    writeln!(out, "LAST WAKE INFORMATION")?;
    writeln!(out, "{}", "=".repeat(50))?;

    // [LAST SLEEP/WAKE CYCLE]
    writeln!(out)?;
    writeln!(out, "[LAST SLEEP/WAKE CYCLE]")?;
    writeln!(out, "{}", "-".repeat(30))?;
    match report.last_sleep {
        Some(t) => writeln!(out, "  Sleep time: {}", format_journal_ts(t))?,
        None => writeln!(out, "  Sleep time: Unknown")?,
    }
    match report.last_wake {
        Some(t) => writeln!(out, "  Wake time:  {}", format_journal_ts(t))?,
        None => writeln!(
            out,
            "  Wake time:  Unknown (system may not have slept this boot)"
        )?,
    }
    if let (Some(sleep), Some(wake)) = (report.last_sleep, report.last_wake) {
        // Both present — render the duration when wake is strictly
        // after sleep. A negative or zero delta means the journal had
        // them out of order (rare; the matcher catches old wake-only
        // boots) and Python silently skips the line.
        let delta = wake.signed_duration_since(sleep);
        if delta.num_seconds() > 0 {
            // chrono::Duration may be negative; we already gated on >0.
            // num_seconds() truncates toward zero — matching Python's
            // `int(td.total_seconds())`.
            let secs = delta.num_seconds() as u64;
            let formatted = format_duration(Duration::from_secs(secs));
            writeln!(out, "  Duration:   {formatted}")?;
        }
    }

    // [WAKE SOURCE]
    writeln!(out)?;
    writeln!(out, "[WAKE SOURCE]")?;
    writeln!(out, "{}", "-".repeat(30))?;
    match &report.wake_irq {
        Some(wi) => {
            writeln!(out, "  Wake IRQ: {}", wi.irq)?;
            if let Some(dev) = &wi.device {
                writeln!(out, "  Device: {dev}")?;
            }
        }
        None => writeln!(out, "  Wake IRQ: Not available")?,
    }

    // [KERNEL WAKE MESSAGES] — verbose only, only if non-empty.
    // Matches Python `if dmesg_wake and args.verbose:` (line 1113):
    // an empty dmesg result skips the section even under -v.
    if verbose {
        if let Some(lines) = &report.dmesg_wake {
            if !lines.is_empty() {
                writeln!(out)?;
                writeln!(out, "[KERNEL WAKE MESSAGES]")?;
                writeln!(out, "{}", "-".repeat(30))?;
                for line in lines {
                    let truncated = truncate_kernel_wake_line(line);
                    writeln!(out, "  {truncated}")?;
                }
            }
        }
    }

    // [ENABLED ACPI WAKE DEVICES] — verbose only, always renders the
    // header (with `  None.` fallback when no devices are enabled).
    if verbose {
        writeln!(out)?;
        writeln!(out, "[ENABLED ACPI WAKE DEVICES]")?;
        writeln!(out, "{}", "-".repeat(30))?;
        match &report.acpi_enabled {
            Some(devs) if !devs.is_empty() => {
                for dev in devs {
                    let sysfs_suffix = match &dev.sysfs {
                        Some(s) if !s.is_empty() => format!(" ({s})"),
                        _ => String::new(),
                    };
                    writeln!(out, "  {}: {}{}", dev.device, dev.state, sysfs_suffix)?;
                }
            }
            _ => writeln!(out, "  None.")?,
        }
    }

    // [RECENT SLEEP/WAKE HISTORY] — history mode only.
    if history_count.is_some() {
        writeln!(out)?;
        writeln!(out, "[RECENT SLEEP/WAKE HISTORY]")?;
        writeln!(out, "{}", "-".repeat(30))?;
        match &report.history {
            Some(events) if !events.is_empty() => {
                for event in events {
                    let kind = match event.kind {
                        SleepEventKind::Sleep => "SLEEP",
                        SleepEventKind::Wake => "WAKE",
                    };
                    writeln!(out, "  {} - {kind}", format_journal_ts(event.time))?;
                }
            }
            _ => writeln!(out, "  No sleep/wake events found.")?,
        }
    }

    writeln!(out)?;
    writeln!(out, "{}", "=".repeat(50))?;
    Ok(())
}

/// Truncate a kernel-wake-message line to 70 chars, replacing the tail
/// with `...` (so the result is still ≤70 chars). Mirrors the Python
/// guard `if len(line) > 70: line = line[:67] + "..."`.
///
/// `chars().take()` ensures we don't split mid-codepoint when a wake
/// message contains non-ASCII bytes (rare on real kernels, but the
/// dmesg buffer can contain anything).
fn truncate_kernel_wake_line(line: &str) -> String {
    if line.chars().count() > KERNEL_WAKE_LINE_MAX {
        let head: String = line.chars().take(KERNEL_WAKE_LINE_MAX - 3).collect();
        format!("{head}...")
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::devicequery::UsbWakeDevice;
    use crate::model::requests::{AudioStream, Inhibitor, ProcessInfo, RequestsReport};
    use crate::model::waketimers::{TimerEntry, WakeTimersReport};

    /// Helper: build a typical report with one sleep/idle inhibitor
    /// (renders) and one shutdown inhibitor (hidden but counted).
    fn typical_report() -> RequestsReport {
        RequestsReport {
            inhibitors: vec![
                Inhibitor {
                    who: "firefox".into(),
                    why: "Playing audio".into(),
                    what: "sleep:idle".into(),
                    mode: "block".into(),
                    uid: 1000,
                    pid: 12345,
                    comm: "firefox".into(),
                },
                Inhibitor {
                    who: "gdm".into(),
                    why: "Saving state".into(),
                    what: "shutdown".into(),
                    mode: "block".into(),
                    uid: 0,
                    pid: 999,
                    comm: "gdm-session".into(),
                },
            ],
            wake_locks: vec!["audio".into()],
            audio_streams: vec![
                // Full-population: name + binary + pid all present.
                AudioStream {
                    id: "123".into(),
                    application_name: Some("Firefox".into()),
                    pid: Some(12345),
                    binary: Some("firefox".into()),
                },
                // Partial: name + pid, no binary (the rare-but-legitimate
                // sandboxed-stream case).
                AudioStream {
                    id: "124".into(),
                    application_name: Some("Spotify".into()),
                    pid: Some(7775),
                    binary: None,
                },
            ],
            vms: vec![ProcessInfo {
                pid: 5678,
                comm: "qemu-system-x86".into(),
            }],
            usb_wakeup: vec![],
        }
    }

    /// Display loop hides the `shutdown` inhibitor but the summary
    /// counts it: total = 2 inhibitors + 1 wake_lock + 2 audio + 1 vm = 6.
    #[test]
    fn print_requests_typical_filters_inhibitors_but_counts_all() {
        let report = typical_report();
        let mut out = Vec::new();
        print_requests(&report, false, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.contains("Total sleep blockers found: 6"),
            "summary should count unfiltered inhibitors: {s}",
        );
        assert!(
            !s.contains("gdm-session"),
            "shutdown inhibitor should not appear in display: {s}",
        );
        assert!(
            !s.contains("[USB WAKEUP DEVICES]"),
            "non-verbose run should omit USB section: {s}",
        );
        insta::assert_snapshot!("print_requests_typical", s);
    }

    /// Verbose adds the USB wakeup section but the count stays at 6
    /// (USB devices are not summed in the total).
    #[test]
    fn print_requests_typical_verbose_adds_usb_section() {
        let mut report = typical_report();
        report.usb_wakeup = vec![UsbWakeDevice {
            device: "1-2".into(),
            name: "Logitech USB Receiver".into(),
        }];
        let mut out = Vec::new();
        print_requests(&report, true, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.contains("Total sleep blockers found: 6"),
            "USB devices must not bump the count: {s}",
        );
        assert!(
            s.contains("[USB WAKEUP DEVICES]"),
            "verbose run should include USB section: {s}",
        );
        assert!(
            s.contains("Logitech USB Receiver (1-2)"),
            "USB device should render as 'name (device)': {s}",
        );
        insta::assert_snapshot!("print_requests_typical_verbose", s);
    }

    /// All-empty report: every section shows `None.` and the trailing
    /// line collapses to `No active sleep blockers detected.` with no
    /// count (matches Python's else branch).
    #[test]
    fn print_requests_empty_renders_no_blockers_message() {
        let report = RequestsReport::default();
        let mut out = Vec::new();
        print_requests(&report, false, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.contains("No active sleep blockers detected."),
            "empty report should print the no-blockers line: {s}",
        );
        assert!(
            !s.contains("Total sleep blockers found"),
            "empty report should NOT include the count line: {s}",
        );
        insta::assert_snapshot!("print_requests_empty", s);
    }

    /// All-non-sleep inhibitors: the section header still prints, the
    /// display-time filter hides every entry, and the summary count
    /// includes them all (so the "Total: 1" line still appears even
    /// though the inhibitor section visibly shows nothing under its
    /// header). Pins the "iterate-all-display-some" contract so a
    /// future regression that swaps to filter-then-iterate would fail.
    #[test]
    fn print_requests_only_non_sleep_inhibitors_still_counts_them() {
        let report = RequestsReport {
            inhibitors: vec![
                Inhibitor {
                    what: "shutdown".into(),
                    who: "gdm".into(),
                    why: "Saving session state".into(),
                    mode: "block".into(),
                    uid: 0,
                    pid: 999,
                    comm: "gdm-session".into(),
                },
                Inhibitor {
                    what: "handle-power-key".into(),
                    who: "logind".into(),
                    why: "Power key handler".into(),
                    mode: "block".into(),
                    uid: 0,
                    pid: 1,
                    comm: "systemd".into(),
                },
            ],
            ..Default::default()
        };
        let mut out = Vec::new();
        print_requests(&report, false, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.contains("[SYSTEM INHIBITORS]"),
            "header should print: {s}"
        );
        assert!(
            !s.contains("Process: gdm-session"),
            "non-sleep inhibitors must not appear in the display: {s}",
        );
        assert!(
            !s.contains("Process: systemd"),
            "non-sleep inhibitors must not appear in the display: {s}",
        );
        assert!(
            s.contains("Total sleep blockers found: 2"),
            "summary must count all inhibitors regardless of display filter: {s}",
        );
        insta::assert_snapshot!("print_requests_only_non_sleep", s);
    }

    // ---- format_audio_client arm coverage -------------------------------
    //
    // One test per arm of the 2x2x2 (name, binary, pid) match. The match
    // arms are exhaustive in the implementation; pinning all 8 here keeps
    // the per-arm format string from drifting accidentally.

    fn stream_with(name: Option<&str>, binary: Option<&str>, pid: Option<u32>) -> AudioStream {
        AudioStream {
            id: "0".into(),
            application_name: name.map(str::to_owned),
            binary: binary.map(str::to_owned),
            pid,
        }
    }

    #[test]
    fn format_audio_client_all_three_present() {
        let s = stream_with(Some("Firefox"), Some("firefox"), Some(12345));
        assert_eq!(format_audio_client(&s), "Firefox (firefox, PID: 12345)");
    }

    #[test]
    fn format_audio_client_name_and_pid_no_binary() {
        let s = stream_with(Some("Spotify"), None, Some(7775));
        assert_eq!(format_audio_client(&s), "Spotify (PID: 7775)");
    }

    #[test]
    fn format_audio_client_name_and_binary_no_pid() {
        // Rare but legitimate: a system stream that exposes binary
        // but no process.id.
        let s = stream_with(Some("Mixer"), Some("pulsemixer"), None);
        assert_eq!(format_audio_client(&s), "Mixer (pulsemixer)");
    }

    #[test]
    fn format_audio_client_name_only() {
        let s = stream_with(Some("Firefox"), None, None);
        assert_eq!(format_audio_client(&s), "Firefox");
    }

    #[test]
    fn format_audio_client_binary_and_pid_no_name() {
        let s = stream_with(None, Some("firefox"), Some(12345));
        assert_eq!(format_audio_client(&s), "firefox (PID: 12345)");
    }

    #[test]
    fn format_audio_client_binary_only() {
        let s = stream_with(None, Some("firefox"), None);
        assert_eq!(format_audio_client(&s), "firefox");
    }

    #[test]
    fn format_audio_client_pid_only() {
        let s = stream_with(None, None, Some(12345));
        assert_eq!(format_audio_client(&s), "PID 12345");
    }

    #[test]
    fn format_audio_client_nothing_known() {
        let s = stream_with(None, None, None);
        assert_eq!(format_audio_client(&s), "<unknown>");
    }

    // ---- print_waketimers tests ----------------------------------------

    /// Tests that mutate `TZ` must serialize — the env is process-global
    /// and `cargo test` runs threads in parallel by default. Mirrors
    /// the same pattern used in `source::sysfs` tests.
    static TZ_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that restores `TZ` on drop. chrono::Local's
    /// `next_elapse_display` formatting is TZ-dependent; tests force
    /// `TZ=UTC` so snapshots stay stable across machines.
    struct TzGuard {
        prev: Option<std::ffi::OsString>,
    }

    impl TzGuard {
        fn capture() -> Self {
            Self {
                prev: std::env::var_os("TZ"),
            }
        }
    }

    impl Drop for TzGuard {
        fn drop(&mut self) {
            // SAFETY: tests holding a TzGuard also hold TZ_LOCK.
            unsafe {
                match self.prev.take() {
                    Some(v) => std::env::set_var("TZ", v),
                    None => std::env::remove_var("TZ"),
                }
            }
        }
    }

    /// SAFETY: caller holds `TZ_LOCK`.
    unsafe fn force_utc() {
        unsafe { std::env::set_var("TZ", "UTC") };
    }

    fn timer(unit: &str, wakes: bool, us: u64) -> TimerEntry {
        TimerEntry {
            unit: unit.to_string(),
            wake_system: wakes,
            next_elapse_realtime_us: us,
        }
    }

    /// Build a report with two wake-capable + three non-wake timers and
    /// an RTC alarm string. µs values chosen for chronological order
    /// in any TZ; tests pin `TZ=UTC` for snapshot stability.
    fn typical_waketimers_report() -> WakeTimersReport {
        // 1745939400 µs-base = 2025-04-29 14:30:00 UTC.
        let snapshot_us = 1_745_939_400_000_000;
        let fwupd_us = 1_746_025_800_000_000;
        let apt_us = 1_745_958_194_000_000;
        let logrotate_us = 1_745_976_000_000_000;
        let man_db_us = 1_746_011_400_000_000;

        let wake_timers = vec![
            timer("snapshot.timer", true, snapshot_us),
            timer("fwupd-refresh.timer", true, fwupd_us),
        ];
        let mut all_timers = wake_timers.clone();
        all_timers.push(timer("apt-daily.timer", false, apt_us));
        all_timers.push(timer("logrotate.timer", false, logrotate_us));
        all_timers.push(timer("man-db.timer", false, man_db_us));

        WakeTimersReport {
            wake_timers,
            all_timers,
            rtc_wakealarm: Some("2026-04-29 06:00:00".to_string()),
        }
    }

    #[test]
    fn print_waketimers_typical() {
        // chrono::Local on Linux uses iana-time-zone (reads
        // /etc/localtime, not $TZ), so the rendered next-elapse strings
        // depend on the test machine's system TZ. We hold TZ_LOCK +
        // TzGuard for symmetry with other env-touching tests, but the
        // assertions are substring-only — they pin the structure (which
        // sections appear, what counts render) without committing to a
        // specific TZ-formatted timestamp.
        let _lock = TZ_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _tz = TzGuard::capture();
        // SAFETY: TZ_LOCK held.
        unsafe { force_utc() };

        let report = typical_waketimers_report();
        let mut out = Vec::new();
        print_waketimers(&report, false, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();

        assert!(s.starts_with("WAKE TIMERS\n"), "header missing: {s}");
        assert!(
            s.contains("[TIMERS WITH WAKESYSTEM=YES]"),
            "wake section missing: {s}",
        );
        assert!(
            s.contains("  snapshot.timer\n    Next:"),
            "wake-timer entry must use 2-line block format: {s}",
        );
        assert!(
            s.contains("  fwupd-refresh.timer\n    Next:"),
            "second wake-timer entry must follow same format: {s}",
        );
        assert!(
            !s.contains("[ALL SCHEDULED TIMERS]"),
            "non-verbose run should omit all-timers table: {s}",
        );
        assert!(
            !s.contains("apt-daily.timer"),
            "non-verbose run should hide non-wake timers: {s}",
        );
        assert!(
            s.contains("[RTC WAKE ALARM]") && s.contains("Scheduled wake: 2026-04-29 06:00:00"),
            "RTC alarm section missing: {s}",
        );
        assert!(
            s.trim_end().ends_with("Wake timers active: 2"),
            "summary line wrong: {s}",
        );
    }

    #[test]
    fn print_waketimers_typical_verbose() {
        let _lock = TZ_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _tz = TzGuard::capture();
        // SAFETY: TZ_LOCK held.
        unsafe { force_utc() };

        let report = typical_waketimers_report();
        let mut out = Vec::new();
        print_waketimers(&report, true, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();

        assert!(
            s.contains("[ALL SCHEDULED TIMERS]"),
            "verbose run should include all-timers table: {s}",
        );
        // Header row uses fixed widths: "Timer" (≤35) "Wakes" (≤6) "Next".
        assert!(
            s.contains("  Timer                               Wakes  Next"),
            "table header row missing or width drifted: {s}",
        );
        assert!(
            s.contains("apt-daily.timer"),
            "non-wake entry should appear under verbose: {s}",
        );
        // Wake-system column renders as Yes / No literally.
        assert!(
            s.contains("snapshot.timer") && s.contains("Yes"),
            "wake-system column should render Yes for wake timers: {s}",
        );
        assert!(
            s.contains("apt-daily.timer") && s.contains("No"),
            "wake-system column should render No for non-wake timers: {s}",
        );
        // 2 Yes (wake) + 3 No (non-wake) = 5 rows total. 5 < 15 so no truncation.
        assert!(
            !s.contains("more timers"),
            "5-entry list should not trigger truncation: {s}",
        );
        assert!(
            s.trim_end().ends_with("Wake timers active: 2"),
            "summary line wrong: {s}",
        );
    }

    #[test]
    fn print_waketimers_empty() {
        // No TZ lock needed — the empty-report path doesn't traverse
        // any timer entries (and so doesn't call next_elapse_display),
        // so the output is fully deterministic.
        let report = WakeTimersReport::default();
        let mut out = Vec::new();
        print_waketimers(&report, false, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();

        let expected = concat!(
            "WAKE TIMERS\n",
            "==================================================\n",
            "\n",
            "[TIMERS WITH WAKESYSTEM=YES]\n",
            "------------------------------\n",
            "  None - no timers will wake the system from sleep\n",
            "\n",
            "[RTC WAKE ALARM]\n",
            "------------------------------\n",
            "  No RTC wake alarm set\n",
            "\n",
            "==================================================\n",
            "No active wake timers\n",
        );
        assert_eq!(s, expected, "empty-report output drifted");
    }

    #[test]
    fn print_waketimers_truncates_at_15() {
        let _lock = TZ_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _tz = TzGuard::capture();
        // SAFETY: TZ_LOCK held.
        unsafe { force_utc() };

        // 20 timers, none wake-capable, so the verbose all-timers table
        // hits the 15-row cap.
        let all_timers: Vec<TimerEntry> = (0..20)
            .map(|i| {
                timer(
                    &format!("timer{i:02}.timer"),
                    false,
                    1_745_939_400_000_000 + (i as u64) * 60_000_000,
                )
            })
            .collect();
        let report = WakeTimersReport {
            wake_timers: vec![],
            all_timers,
            rtc_wakealarm: None,
        };

        let mut out = Vec::new();
        print_waketimers(&report, true, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();

        // First 15 (00..14) appear; 15..19 are truncated.
        assert!(s.contains("timer00.timer"), "first row missing: {s}");
        assert!(s.contains("timer14.timer"), "15th row missing: {s}");
        assert!(
            !s.contains("timer15.timer"),
            "16th row must be truncated: {s}",
        );
        assert!(
            s.contains("... and 5 more timers"),
            "truncation suffix missing: {s}",
        );
        // Empty wake_timers + non-empty all_timers: still get the
        // "no wake timers" header line, but the all-timers table renders.
        assert!(
            s.contains("None - no timers will wake the system from sleep"),
            "wake-timer fallback missing: {s}",
        );
        assert!(
            s.trim_end().ends_with("No active wake timers"),
            "summary should be no-active line when wake_timers empty: {s}",
        );
    }

    // ---- print_lastwake tests ------------------------------------------

    use crate::model::lastwake::{LastWakeReport, SleepEvent, SleepEventKind, WakeIrq};
    use chrono::{DateTime, FixedOffset};

    fn ts(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z").expect("valid ts")
    }

    #[test]
    fn print_lastwake_default_with_full_data() {
        // Both timestamps + IRQ + device. Duration = 7h 7m 26s.
        let report = LastWakeReport {
            last_sleep: Some(ts("2025-04-29T01:15:18-0700")),
            last_wake: Some(ts("2025-04-29T08:22:44-0700")),
            wake_irq: Some(WakeIrq {
                irq: "9".into(),
                device: Some("9-fasteoi acpi".into()),
            }),
            ..Default::default()
        };
        let mut out = Vec::new();
        print_lastwake(&report, false, None, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Sleep time: 2025-04-29T01:15:18-0700"), "{s}");
        assert!(s.contains("Wake time:  2025-04-29T08:22:44-0700"), "{s}");
        assert!(s.contains("Duration:   7h 7m 26s"), "duration wrong: {s}");
        assert!(s.contains("Wake IRQ: 9"), "{s}");
        assert!(s.contains("Device: 9-fasteoi acpi"), "{s}");
        assert!(
            !s.contains("[KERNEL WAKE MESSAGES]"),
            "non-verbose run should omit kernel wake section: {s}",
        );
        assert!(
            !s.contains("[ENABLED ACPI WAKE DEVICES]"),
            "non-verbose run should omit ACPI section: {s}",
        );
        assert!(
            !s.contains("[RECENT SLEEP/WAKE HISTORY]"),
            "no history flag should omit history section: {s}",
        );
    }

    #[test]
    fn print_lastwake_unknown_when_no_data() {
        let report = LastWakeReport::default();
        let mut out = Vec::new();
        print_lastwake(&report, false, None, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Sleep time: Unknown"), "{s}");
        assert!(
            s.contains("Wake time:  Unknown (system may not have slept this boot)"),
            "{s}",
        );
        assert!(s.contains("Wake IRQ: Not available"), "{s}");
        assert!(
            !s.contains("Duration:"),
            "no timestamps means no duration line: {s}",
        );
    }

    #[test]
    fn print_lastwake_skips_duration_when_wake_before_sleep() {
        // Out-of-order pair (wake before sleep) — Python silently
        // skips the duration line.
        let report = LastWakeReport {
            last_sleep: Some(ts("2025-04-29T08:22:44-0700")),
            last_wake: Some(ts("2025-04-29T01:15:18-0700")),
            ..Default::default()
        };
        let mut out = Vec::new();
        print_lastwake(&report, false, None, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(!s.contains("Duration:"), "{s}");
    }

    #[test]
    fn print_lastwake_verbose_renders_kernel_and_acpi_sections() {
        let report = LastWakeReport {
            last_sleep: Some(ts("2025-04-29T01:15:18-0700")),
            last_wake: Some(ts("2025-04-29T08:22:44-0700")),
            wake_irq: Some(WakeIrq {
                irq: "9".into(),
                device: None,
            }),
            dmesg_wake: Some(vec![
                "2025-04-29T08:22:44-0700 ACPI: Wakeup Device [LID0]".into(),
            ]),
            acpi_enabled: Some(vec![
                crate::model::devicequery::AcpiWakeDevice {
                    device: "GPP0".into(),
                    state: "S4".into(),
                    enabled: true,
                    sysfs: Some("pci:0000:00:01.1".into()),
                },
                crate::model::devicequery::AcpiWakeDevice {
                    device: "PWRB".into(),
                    state: "S4".into(),
                    enabled: true,
                    sysfs: None,
                },
            ]),
            ..Default::default()
        };
        let mut out = Vec::new();
        print_lastwake(&report, true, None, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("[KERNEL WAKE MESSAGES]"), "{s}");
        assert!(s.contains("ACPI: Wakeup Device [LID0]"), "{s}");
        assert!(s.contains("[ENABLED ACPI WAKE DEVICES]"), "{s}");
        assert!(s.contains("  GPP0: S4 (pci:0000:00:01.1)"), "{s}");
        assert!(
            s.contains("  PWRB: S4\n"),
            "PWRB has no sysfs — must render bare: {s}",
        );
        // No Device line: irq.device is None.
        assert!(!s.contains("Device:"), "{s}");
    }

    #[test]
    fn print_lastwake_verbose_acpi_empty_renders_none() {
        let report = LastWakeReport {
            acpi_enabled: Some(vec![]),
            ..Default::default()
        };
        let mut out = Vec::new();
        print_lastwake(&report, true, None, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("[ENABLED ACPI WAKE DEVICES]"), "{s}");
        assert!(s.contains("  None."), "{s}");
    }

    #[test]
    fn print_lastwake_verbose_omits_kernel_section_when_empty() {
        // Verbose flag but no dmesg lines — section omitted entirely.
        let report = LastWakeReport {
            dmesg_wake: Some(vec![]),
            ..Default::default()
        };
        let mut out = Vec::new();
        print_lastwake(&report, true, None, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(!s.contains("[KERNEL WAKE MESSAGES]"), "{s}");
    }

    #[test]
    fn print_lastwake_history_renders_events() {
        let report = LastWakeReport {
            history: Some(vec![
                SleepEvent {
                    time: ts("2025-04-29T01:15:18-0700"),
                    kind: SleepEventKind::Sleep,
                },
                SleepEvent {
                    time: ts("2025-04-29T08:22:44-0700"),
                    kind: SleepEventKind::Wake,
                },
            ]),
            ..Default::default()
        };
        let mut out = Vec::new();
        print_lastwake(&report, false, Some(5), &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("[RECENT SLEEP/WAKE HISTORY]"), "{s}");
        assert!(s.contains("2025-04-29T01:15:18-0700 - SLEEP"), "{s}");
        assert!(s.contains("2025-04-29T08:22:44-0700 - WAKE"), "{s}");
    }

    #[test]
    fn print_lastwake_history_empty_renders_no_events() {
        let report = LastWakeReport {
            history: Some(vec![]),
            ..Default::default()
        };
        let mut out = Vec::new();
        print_lastwake(&report, false, Some(5), &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("[RECENT SLEEP/WAKE HISTORY]"), "{s}");
        assert!(s.contains("  No sleep/wake events found."), "{s}");
    }

    #[test]
    fn truncate_kernel_wake_line_no_truncation_under_70() {
        let line = "x".repeat(70);
        assert_eq!(truncate_kernel_wake_line(&line), line);
    }

    #[test]
    fn truncate_kernel_wake_line_truncates_at_67_with_ellipsis() {
        // 71 chars in → 67 head + "..." = 70 chars out.
        let line = "x".repeat(71);
        let truncated = truncate_kernel_wake_line(&line);
        assert_eq!(truncated.len(), 70);
        assert!(truncated.ends_with("..."));
        assert_eq!(truncated.chars().filter(|&c| c == 'x').count(), 67);
    }
}
