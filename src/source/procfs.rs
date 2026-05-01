//! procfs readers for `/proc`.
//!
//! Phase 1 only needs `/proc/swaps`; later phases add `/proc/interrupts`,
//! `/proc/acpi/wakeup`, and process discovery via `/proc/<pid>/comm`.
//! Phase 3.3 added [`find_processes_by_comm`], which walks `/proc/`
//! directly in favor of shelling out to a process-listing tool — see
//! Designs/RustRewrite/README.md Decision 3a.

use std::fs;

use serde::Serialize;

use crate::model::devicequery::AcpiWakeDevice;
use crate::model::requests::ProcessInfo;
use crate::paths::SysRoot;
use crate::source::SourceError;

/// One swap area as reported by `/proc/swaps`.
///
/// `kind` is the literal value of the `Type` column (`"partition"`,
/// `"file"`, etc.) — left as a free-form string because zram, btrfs swap
/// files, and dm-crypt-on-LVM all show up here and the kernel's exact
/// vocabulary isn't worth pinning to an enum at this layer. JSON output
/// renames it to `"type"` to match the design schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SwapDevice {
    pub device: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub size_kb: u64,
}

/// Read and parse `/proc/swaps`.
///
/// The file has a one-line header followed by whitespace-separated columns:
/// `Filename Type Size Used Priority`. A header-only file (no swap
/// configured) yields `Ok(vec![])`. Lines with fewer than three columns or
/// a non-numeric size are skipped (matches the Python `try/except: pass`
/// around `int(parts[2])`). The file being unreadable returns
/// `SourceError::Io`.
pub fn read_swaps(root: &SysRoot) -> Result<Vec<SwapDevice>, SourceError> {
    let path = root.join("proc/swaps");
    let content = fs::read_to_string(&path)?;
    Ok(parse_swaps(&content))
}

/// Read and parse `/proc/acpi/wakeup`.
///
/// Wraps [`parse_acpi_wakeup`]; an unreadable file (missing on systems
/// without ACPI, permission-denied on some setups) returns
/// `SourceError::Io`.
pub fn read_acpi_wakeup(root: &SysRoot) -> Result<Vec<AcpiWakeDevice>, SourceError> {
    let path = root.join("proc/acpi/wakeup");
    let content = fs::read_to_string(&path)?;
    parse_acpi_wakeup(&content)
}

/// Look up the device-name region of an IRQ row in `/proc/interrupts`.
///
/// `/proc/interrupts` rows look like:
///
/// ```text
///   8:    0    0   IO-APIC   8-edge      rtc0
///   9:  117  204   IO-APIC   9-fasteoi   acpi
/// ```
///
/// We find the line whose first whitespace-trimmed token is `<irq>:`
/// and return the last two tokens joined with a single space (the
/// device-name region). Returns `Ok(None)` when no such line is found.
/// Mirrors Python `get_irq_info` (powercfg.py 299-311); the Rust caller
/// in `cmd::lastwake` swallows any `SourceError::Io` itself.
pub fn read_irq_info(root: &SysRoot, irq: &str) -> Result<Option<String>, SourceError> {
    let path = root.join("proc/interrupts");
    let content = fs::read_to_string(&path)?;
    Ok(find_irq_info(&content, irq))
}

/// Pure parser for [`read_irq_info`]. Walks lines, finds the row whose
/// leading token (after trim) is `<irq>:`, and returns its last two
/// whitespace-separated tokens joined with a space.
fn find_irq_info(content: &str, irq: &str) -> Option<String> {
    let prefix = format!("{irq}:");
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with(&prefix) {
            continue;
        }
        // Confirm the prefix is followed by whitespace (or end-of-line)
        // so `1:` doesn't match `10:`.
        let after = &trimmed[prefix.len()..];
        if !after.is_empty() && !after.starts_with(|c: char| c.is_whitespace()) {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 3 {
            // Python: " ".join(parts[-2:]). The leading "<irq>:" is
            // parts[0], so we always have at least three tokens before
            // taking the last two.
            return Some(parts[parts.len() - 2..].join(" "));
        }
        // 2-token row (e.g. an unfinished kernel emit) — Python guards
        // with `if len(parts) >= 2: return " ".join(parts[-2:])`. The
        // resulting "<irq>: <one-token>" join is not useful; mirror
        // Python's literal behavior.
        if parts.len() == 2 {
            return Some(parts.join(" "));
        }
        return None;
    }
    None
}

/// Parse the textual `/proc/acpi/wakeup` format.
///
/// The kernel emits a single header line followed by one whitespace-
/// separated row per wake-capable device:
///
/// ```text
/// Device  S-state   Status   Sysfs node
/// GPP0      S4    *enabled   pci:0000:00:01.1
/// PWRB      S4    *enabled
/// ```
///
/// Columns are `device`, `state`, `status`, and an optional `sysfs`
/// node. The status column carries an optional `*` prefix indicating
/// "currently capable of waking" — for our purposes we collapse
/// `*enabled` and bare `enabled` into `enabled = true`, and likewise
/// for `disabled`.
///
/// Behavior:
/// - Empty input or header-only file → `Ok(vec![])`.
/// - Rows with fewer than three columns are skipped (matches the
///   Python `if len(parts) >= 3` guard) — this keeps the parser
///   resilient to occasional kernel quirks rather than failing the
///   whole report.
/// - If the body has at least one non-blank row but none parse, return
///   `SourceError::Parse(...)` so the caller can distinguish a
///   genuinely broken file from "no wake devices configured".
fn parse_acpi_wakeup_inner(content: &str) -> (Vec<AcpiWakeDevice>, usize) {
    // Skip the header line. `lines()` on an empty string yields zero
    // items, so `.skip(1)` on it is still well-defined.
    let mut body_nonblank = 0usize;
    let devices: Vec<AcpiWakeDevice> = content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            body_nonblank += 1;
            let mut parts = trimmed.split_whitespace();
            let device = parts.next()?;
            let state = parts.next()?;
            let status_raw = parts.next()?;
            // The Python version does parts[3] if len(parts) > 3, so a
            // 3-column row yields no sysfs.
            let sysfs = parts.next().map(|s| s.to_owned());
            // Strip a single leading `*`; the kernel never emits more
            // than one but `trim_start_matches` is harmless if it does.
            let status = status_raw.trim_start_matches('*');
            let enabled = status == "enabled";
            Some(AcpiWakeDevice {
                device: device.to_owned(),
                state: state.to_owned(),
                enabled,
                sysfs,
            })
        })
        .collect();
    (devices, body_nonblank)
}

pub fn parse_acpi_wakeup(content: &str) -> Result<Vec<AcpiWakeDevice>, SourceError> {
    let (devices, body_nonblank) = parse_acpi_wakeup_inner(content);
    if devices.is_empty() && body_nonblank > 0 {
        return Err(SourceError::Parse(format!(
            "acpi wakeup: {body_nonblank} non-blank row(s) but none parsed"
        )));
    }
    Ok(devices)
}

fn parse_swaps(content: &str) -> Vec<SwapDevice> {
    content
        .lines()
        .skip(1) // header
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let device = parts.next()?;
            let kind = parts.next()?;
            let size_kb = parts.next()?.parse::<u64>().ok()?;
            Some(SwapDevice {
                device: device.to_owned(),
                kind: kind.to_owned(),
                size_kb,
            })
        })
        .collect()
}

/// Walk `<root>/proc/`, find processes whose `/proc/<pid>/comm` matches
/// any of the given names, and return them as `ProcessInfo`.
///
/// Replaces the Python tool's process-listing shellouts (e.g. for
/// `qemu`, `VBoxHeadless`) with a direct directory walk (Decision 3a).
///
/// Filtering is exact-match against the trimmed `comm` content. The
/// kernel truncates `comm` at 15 characters, so caller-supplied names
/// must respect that limit — pass `"qemu-system-x86"` rather than the
/// full `"qemu-system-x86_64"`. The 15-char cap is enforced silently by
/// the kernel; passing a longer name simply produces no matches.
///
/// Behavior:
/// - Non-numeric direntries (`net`, `cpuinfo`, `self`, etc.) are
///   silently skipped.
/// - Unreadable `comm` files (process exited mid-walk, permission
///   denied) are silently skipped — their PID is omitted from the
///   result.
/// - An empty `names` slice short-circuits to `Ok(vec![])` without
///   walking the directory.
/// - Returns `Err(SourceError::Io)` only if `<root>/proc/` itself can't
///   be enumerated, which is virtually impossible on a running Linux
///   system but happens in tests when the fixture omits `proc/`.
pub fn find_processes_by_comm(
    root: &SysRoot,
    names: &[&str],
) -> Result<Vec<ProcessInfo>, SourceError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    // The kernel's task_struct::comm field is TASK_COMM_LEN = 16 bytes
    // (NUL-terminated → 15 visible chars). Names longer than that can
    // never match anything because the kernel will have truncated the
    // comm value before we read it. Catch the misconfiguration loudly
    // in debug builds so wiring sites in cmd::* discover it during
    // tests, not in production silence.
    debug_assert!(
        names.iter().all(|n| n.len() <= 15),
        "comm name longer than kernel's 15-char limit: {:?}",
        names.iter().find(|n| n.len() > 15),
    );

    let proc_dir = root.join("proc");
    let entries = fs::read_dir(&proc_dir)?;

    let mut found = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name_str) = file_name.to_str() else {
            continue;
        };
        let Ok(pid) = name_str.parse::<u32>() else {
            // Non-numeric direntries (net, cpuinfo, self, ...) are
            // silently skipped — only numeric PID dirs interest us.
            continue;
        };

        let comm_path = entry.path().join("comm");
        let Ok(raw) = fs::read_to_string(&comm_path) else {
            // Process exited mid-walk, permission denied, or
            // /proc/<pid>/comm absent — silently skip.
            continue;
        };
        let comm = raw.trim_end_matches('\n').trim_end().to_owned();
        if names.iter().any(|n| *n == comm) {
            found.push(ProcessInfo { pid, comm });
        }
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER_ONLY: &str = "Filename                                Type            Size            Used            Priority\n";

    const ZRAM_AND_PARTITION: &str = "Filename                                Type            Size            Used            Priority
/dev/dm-0                               partition       16777212        0               -2
/dev/zram0                              partition       8388604         0               5
";

    #[test]
    fn parse_swaps_header_only_yields_empty() {
        assert!(parse_swaps(HEADER_ONLY).is_empty());
    }

    #[test]
    fn parse_swaps_two_entries() {
        let swaps = parse_swaps(ZRAM_AND_PARTITION);
        assert_eq!(swaps.len(), 2);
        assert_eq!(
            swaps[0],
            SwapDevice {
                device: "/dev/dm-0".into(),
                kind: "partition".into(),
                size_kb: 16_777_212,
            }
        );
        assert_eq!(
            swaps[1],
            SwapDevice {
                device: "/dev/zram0".into(),
                kind: "partition".into(),
                size_kb: 8_388_604,
            }
        );
    }

    #[test]
    fn parse_swaps_skips_malformed_line() {
        // Second data line has only one column (the device path) and no
        // type/size — must be skipped without affecting the well-formed
        // entries before/after it.
        let content = "Filename Type Size Used Priority
/dev/dm-0 partition 16777212 0 -2
/dev/short
/dev/zram0 partition 8388604 0 5
";
        let swaps = parse_swaps(content);
        assert_eq!(swaps.len(), 2);
        assert_eq!(swaps[0].device, "/dev/dm-0");
        assert_eq!(swaps[1].device, "/dev/zram0");
    }

    #[test]
    fn parse_swaps_skips_non_numeric_size() {
        let content = "Filename Type Size Used Priority
/dev/dm-0 partition not-a-number 0 -2
/dev/zram0 partition 8388604 0 5
";
        let swaps = parse_swaps(content);
        assert_eq!(swaps.len(), 1);
        assert_eq!(swaps[0].device, "/dev/zram0");
    }

    #[test]
    fn read_swaps_missing_is_io_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let err = read_swaps(&root).expect_err("missing file should error");
        assert!(matches!(err, SourceError::Io(_)));
    }

    #[test]
    fn read_swaps_via_fixture_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let path = root.join("proc/swaps");
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        std::fs::write(&path, ZRAM_AND_PARTITION).expect("write");
        let swaps = read_swaps(&root).expect("read");
        assert_eq!(swaps.len(), 2);
    }

    const WAKEUP_HEADER: &str = "Device  S-state   Status   Sysfs node\n";

    const WAKEUP_TYPICAL: &str = "Device  S-state   Status   Sysfs node
GPP0      S4    *enabled   pci:0000:00:01.1
GPP8      S4    *disabled  pci:0000:00:08.1
PWRB      S4    *enabled
";

    #[test]
    fn parse_acpi_wakeup_header_only_yields_empty() {
        let devices = parse_acpi_wakeup(WAKEUP_HEADER).expect("parse");
        assert!(devices.is_empty());
    }

    #[test]
    fn parse_acpi_wakeup_completely_empty_yields_empty() {
        let devices = parse_acpi_wakeup("").expect("parse");
        assert!(devices.is_empty());
    }

    #[test]
    fn parse_acpi_wakeup_star_enabled_and_bare_enabled_both_true() {
        let content = "Device  S-state   Status   Sysfs node
GPP0      S4    *enabled   pci:0000:00:01.1
LID0      S3     enabled   platform:PNP0C0D:00
";
        let devices = parse_acpi_wakeup(content).expect("parse");
        assert_eq!(devices.len(), 2);
        assert!(devices[0].enabled, "*enabled should map to true");
        assert!(devices[1].enabled, "bare enabled should also map to true");
    }

    #[test]
    fn parse_acpi_wakeup_star_disabled_is_false() {
        let content = "Device  S-state   Status   Sysfs node
GPP8      S4    *disabled  pci:0000:00:08.1
";
        let devices = parse_acpi_wakeup(content).expect("parse");
        assert_eq!(devices.len(), 1);
        assert!(!devices[0].enabled);
    }

    #[test]
    fn parse_acpi_wakeup_three_columns_has_no_sysfs() {
        let content = "Device  S-state   Status   Sysfs node
PWRB      S4    *enabled
";
        let devices = parse_acpi_wakeup(content).expect("parse");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device, "PWRB");
        assert_eq!(devices[0].state, "S4");
        assert!(devices[0].enabled);
        assert_eq!(devices[0].sysfs, None);
    }

    #[test]
    fn parse_acpi_wakeup_typical_input_has_three_devices() {
        let devices = parse_acpi_wakeup(WAKEUP_TYPICAL).expect("parse");
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].device, "GPP0");
        assert_eq!(devices[0].sysfs.as_deref(), Some("pci:0000:00:01.1"));
        assert!(devices[0].enabled);
        assert!(!devices[1].enabled);
        assert_eq!(devices[2].sysfs, None);
    }

    #[test]
    fn parse_acpi_wakeup_skips_short_rows_without_failing() {
        // Mix of one valid row and a 2-column malformed row. Per the
        // Python parser's `len(parts) >= 3` guard the short row should
        // be silently dropped while the valid row is preserved.
        let content = "Device  S-state   Status   Sysfs node
GPP0      S4    *enabled   pci:0000:00:01.1
SHORT     S4
";
        let devices = parse_acpi_wakeup(content).expect("parse");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device, "GPP0");
    }

    #[test]
    fn parse_acpi_wakeup_all_rows_malformed_returns_parse_error() {
        // Non-empty body where every row is too short to parse — the
        // file is structurally broken; the caller wants to know.
        let content = "Device  S-state   Status   Sysfs node
GPP0
GPP8 S4
";
        let err = parse_acpi_wakeup(content).expect_err("expected parse error");
        assert!(matches!(err, SourceError::Parse(_)));
    }

    #[test]
    fn read_acpi_wakeup_via_fixture_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let path = root.join("proc/acpi/wakeup");
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        std::fs::write(&path, WAKEUP_TYPICAL).expect("write");
        let devices = read_acpi_wakeup(&root).expect("read");
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].device, "GPP0");
    }

    #[test]
    fn read_acpi_wakeup_missing_is_io_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let err = read_acpi_wakeup(&root).expect_err("missing file should error");
        assert!(matches!(err, SourceError::Io(_)));
    }

    #[test]
    fn read_acpi_wakeup_typical_fixture_tree() {
        // Exercise the committed fixture (in addition to the inline
        // tempdir version above) to lock in the on-disk layout. Assert
        // the exact row count + first field so a fixture edit that
        // accidentally drops or reorders a row fails the test loudly.
        let root = SysRoot::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sys-typical",
        ));
        let devices = read_acpi_wakeup(&root).expect("read fixture");
        assert_eq!(devices.len(), 3, "fixture has 3 rows: GPP0, GPP8, PWRB");
        assert_eq!(devices[0].device, "GPP0");
        assert!(devices[0].enabled);
        assert_eq!(devices[1].device, "GPP8");
        assert!(!devices[1].enabled, "GPP8 marked *disabled in fixture");
        assert_eq!(devices[2].device, "PWRB");
        assert!(devices[2].sysfs.is_none(), "PWRB has no sysfs column");
    }

    /// Helper for the `/proc` walk tests: build a fake `<root>/proc/<pid>/comm`.
    fn write_comm(root: &SysRoot, pid_dir: &str, comm: &str) {
        let path = root.join(format!("proc/{pid_dir}/comm"));
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        std::fs::write(&path, comm).expect("write comm");
    }

    #[test]
    fn find_processes_by_comm_returns_only_matching_pids() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_comm(&root, "1234", "qemu-system-x86\n");
        write_comm(&root, "5678", "bash\n");
        let found =
            find_processes_by_comm(&root, &["qemu-system-x86"]).expect("walk should succeed");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pid, 1234);
        assert_eq!(found[0].comm, "qemu-system-x86");
    }

    #[test]
    fn find_processes_by_comm_skips_non_numeric_direntries() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        // Layout `/proc/` with a real numeric PID dir plus the kinds of
        // non-numeric children the kernel always exposes (net, cpuinfo,
        // self) so the walk has to filter them out without erroring.
        write_comm(&root, "42", "qemu-system-x86\n");
        std::fs::create_dir_all(root.join("proc/net")).expect("mkdir net");
        std::fs::write(root.join("proc/cpuinfo"), b"model name : x\n").expect("cpuinfo");
        std::fs::create_dir_all(root.join("proc/self")).expect("mkdir self");

        let found =
            find_processes_by_comm(&root, &["qemu-system-x86"]).expect("walk should succeed");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pid, 42);
    }

    #[test]
    fn find_processes_by_comm_skips_pid_dir_without_comm_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        // PID dir exists but has no `comm` file (process raced to exit
        // between the directory listing and the read). Must be silently
        // skipped, not surfaced as an error.
        std::fs::create_dir_all(root.join("proc/9999")).expect("mkdir");
        write_comm(&root, "1234", "qemu-system-x86\n");

        let found = find_processes_by_comm(&root, &["qemu-system-x86"]).expect("walk");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pid, 1234);
    }

    #[test]
    fn find_processes_by_comm_trims_trailing_newline() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        // The kernel always appends `\n` to /proc/<pid>/comm. Make sure
        // the trim happens before the equality check.
        write_comm(&root, "1", "bash\n");
        let found = find_processes_by_comm(&root, &["bash"]).expect("walk");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].comm, "bash");
    }

    #[test]
    fn find_processes_by_comm_excludes_non_matching_comm() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_comm(&root, "1", "bash\n");
        write_comm(&root, "2", "zsh\n");
        let found = find_processes_by_comm(&root, &["fish"]).expect("walk");
        assert!(found.is_empty(), "no match should yield empty Vec");
    }

    #[test]
    fn find_processes_by_comm_empty_names_returns_empty_without_walking() {
        // Empty names slice short-circuits — `proc/` doesn't even need
        // to exist for the call to succeed.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let found = find_processes_by_comm(&root, &[]).expect("empty names is Ok");
        assert!(found.is_empty());
    }

    #[test]
    fn find_processes_by_comm_missing_proc_dir_is_io_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        // No `proc/` under the fixture root — read_dir surfaces ENOENT.
        let err = find_processes_by_comm(&root, &["bash"]).expect_err("missing proc/ should error");
        assert!(matches!(err, SourceError::Io(_)));
    }

    #[test]
    fn find_processes_by_comm_returns_multiple_matches_for_same_name() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        // Two PIDs both running qemu — both must come back. Order
        // depends on the filesystem's directory iteration so the test
        // sorts by PID before asserting.
        write_comm(&root, "1001", "qemu-system-x86\n");
        write_comm(&root, "1002", "qemu-system-x86\n");
        let mut found = find_processes_by_comm(&root, &["qemu-system-x86"]).expect("walk");
        found.sort_by_key(|p| p.pid);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].pid, 1001);
        assert_eq!(found[1].pid, 1002);
    }

    // ---- find_irq_info / read_irq_info tests ---------------------------

    const PROC_INTERRUPTS_TYPICAL: &str = "           CPU0       CPU1
  0:        125          0   IO-APIC    2-edge      timer
  8:          1          0   IO-APIC    8-edge      rtc0
  9:        117        204   IO-APIC    9-fasteoi   acpi
 14:        500        300   IO-APIC   14-edge      ata_piix
NMI:          0          0   Non-maskable interrupts
";

    #[test]
    fn find_irq_info_returns_last_two_tokens_for_match() {
        // IRQ 9: trailing tokens are "9-fasteoi" and "acpi".
        let info = find_irq_info(PROC_INTERRUPTS_TYPICAL, "9").expect("match");
        assert_eq!(info, "9-fasteoi acpi");
    }

    #[test]
    fn find_irq_info_returns_none_for_missing_irq() {
        assert!(find_irq_info(PROC_INTERRUPTS_TYPICAL, "999").is_none());
    }

    #[test]
    fn find_irq_info_does_not_match_prefix() {
        // IRQ "1" must NOT match the line beginning "14:".
        assert!(find_irq_info(PROC_INTERRUPTS_TYPICAL, "1").is_none());
    }

    #[test]
    fn find_irq_info_skips_non_numeric_irq_label_rows() {
        // The "NMI:" row starts with a non-numeric prefix; an IRQ lookup
        // for "NMI" would technically match (Python doesn't guard on
        // numeric-only either). Pin the literal behavior so a refactor
        // doesn't accidentally diverge.
        let info = find_irq_info(PROC_INTERRUPTS_TYPICAL, "NMI").expect("match");
        assert_eq!(info, "Non-maskable interrupts");
    }

    #[test]
    fn find_irq_info_two_token_row_returns_label_and_token() {
        // 2-token rows are unusual but Python's `" ".join(parts[-2:])`
        // returns "<irq>: <token>" verbatim. Lock the behavior so the
        // 2-token branch in find_irq_info doesn't drift unnoticed.
        let input = "  9:        acpi\n";
        let info = find_irq_info(input, "9").expect("match");
        assert_eq!(info, "9: acpi");
    }

    #[test]
    fn read_irq_info_via_fixture_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let path = root.join("proc/interrupts");
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        std::fs::write(&path, PROC_INTERRUPTS_TYPICAL).expect("write");
        let info = read_irq_info(&root, "8").expect("read").expect("match");
        assert_eq!(info, "8-edge rtc0");
    }

    #[test]
    fn read_irq_info_missing_file_is_io_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let err = read_irq_info(&root, "9").expect_err("missing file should error");
        assert!(matches!(err, SourceError::Io(_)));
    }

    #[test]
    fn find_processes_by_comm_matches_any_in_names_list() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        // Names list with two entries — `cmd::requests` will pass
        // `["qemu-system-x86", "VBoxHeadless"]` together.
        write_comm(&root, "10", "qemu-system-x86\n");
        write_comm(&root, "20", "VBoxHeadless\n");
        write_comm(&root, "30", "bash\n");
        let mut found =
            find_processes_by_comm(&root, &["qemu-system-x86", "VBoxHeadless"]).expect("walk");
        found.sort_by_key(|p| p.pid);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].comm, "qemu-system-x86");
        assert_eq!(found[1].comm, "VBoxHeadless");
    }
}
