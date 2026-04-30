//! procfs readers for `/proc`.
//!
//! Phase 1 only needs `/proc/swaps`; later phases add `/proc/interrupts`,
//! `/proc/acpi/wakeup`, and process discovery via `/proc/<pid>/comm`.

use std::fs;

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
}
