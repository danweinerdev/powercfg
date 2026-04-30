//! procfs readers for `/proc`.
//!
//! Phase 1 only needs `/proc/swaps`; later phases add `/proc/interrupts`,
//! `/proc/acpi/wakeup`, and process discovery via `/proc/<pid>/comm`.

use std::fs;

use crate::model::devicequery::AcpiWakeDevice;
use crate::paths::SysRoot;
use crate::source::SourceError;

/// One swap area as reported by `/proc/swaps`.
///
/// `kind` is the literal value of the `Type` column (`"partition"`,
/// `"file"`, etc.) — left as a free-form string because zram, btrfs swap
/// files, and dm-crypt-on-LVM all show up here and the kernel's exact
/// vocabulary isn't worth pinning to an enum at this layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapDevice {
    pub device: String,
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
// TODO(phase-2.2): wired by cmd::devicequery::run; tests reach it now,
// the bin build doesn't until that lands.
#[allow(dead_code)]
pub fn read_acpi_wakeup(root: &SysRoot) -> Result<Vec<AcpiWakeDevice>, SourceError> {
    let path = root.join("proc/acpi/wakeup");
    let content = fs::read_to_string(&path)?;
    parse_acpi_wakeup(&content)
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
#[allow(dead_code)]
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

#[allow(dead_code)]
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
}
