//! sysfs readers for `/sys`.
//!
//! Functions take a `&SysRoot` so integration tests can point at fixture
//! trees (`POWERCFG_SYSROOT=tests/fixtures/sys-typical`). Every reader
//! returns `Result<T, SourceError>`; the command layer decides whether a
//! missing file or unreadable permission should render as "no data" or as
//! a hard error.

use std::fs;
use std::path::PathBuf;

use crate::model::devicequery::WakeupStats;
use crate::paths::SysRoot;
use crate::source::SourceError;

/// Read `/sys/power/state` — a space-separated list of the kernel-supported
/// sleep state tokens (e.g. `freeze mem disk`).
///
/// An empty file yields `Ok(vec![])`. I/O failures (missing file,
/// permission denied) propagate as `SourceError::Io`.
pub fn read_sleep_states(root: &SysRoot) -> Result<Vec<String>, SourceError> {
    let path = root.join("sys/power/state");
    let content = fs::read_to_string(&path)?;
    Ok(content.split_whitespace().map(|s| s.to_owned()).collect())
}

/// Read `/sys/power/mem_sleep`, parsing the bracketed-current syntax.
///
/// The kernel writes this file as a space-separated list of supported
/// memory-sleep modes with the currently selected one wrapped in square
/// brackets, e.g. `[s2idle] deep`. The bracket marker is stripped from
/// the mode name when added to the modes list (matching Python's
/// `get_mem_sleep_modes`); the bare value is returned as the second tuple
/// element.
///
/// Returns `(modes, current)` where `current` is `None` if no bracketed
/// token was present.
pub fn read_mem_sleep_modes(root: &SysRoot) -> Result<(Vec<String>, Option<String>), SourceError> {
    let path = root.join("sys/power/mem_sleep");
    let content = fs::read_to_string(&path)?;
    Ok(parse_bracketed_modes(&content))
}

/// Read `/sys/power/disk` (hibernation mode), parsing the bracketed-current
/// syntax. Same shape as [`read_mem_sleep_modes`].
pub fn read_disk_modes(root: &SysRoot) -> Result<(Vec<String>, Option<String>), SourceError> {
    let path = root.join("sys/power/disk");
    let content = fs::read_to_string(&path)?;
    Ok(parse_bracketed_modes(&content))
}

/// Read `/sys/power/image_size` — the maximum hibernation image size in
/// bytes as a single decimal integer.
///
/// Missing file → `SourceError::Io` (the caller maps this to "section
/// absent"). A non-numeric or empty value → `SourceError::Parse` so the
/// failure is distinguishable in tests.
pub fn read_image_size_bytes(root: &SysRoot) -> Result<u64, SourceError> {
    let path = root.join("sys/power/image_size");
    let content = fs::read_to_string(&path)?;
    let trimmed = content.trim();
    trimmed
        .parse::<u64>()
        .map_err(|e| SourceError::Parse(format!("image_size: {e} (input: {trimmed:?})")))
}

/// Parse the kernel's bracketed-current syntax into `(modes, current)`.
///
/// Mirrors Python `get_mem_sleep_modes`/`get_disk_modes`: the bracketed
/// token is treated as both a member of the modes list (with brackets
/// stripped) and the current marker. Whitespace-only or empty input
/// returns `(vec![], None)`. Unmatched/partial brackets fall back to the
/// raw token, matching the Python `startswith("[") and endswith("]")`
/// guard.
///
/// If multiple bracketed tokens appear (which the kernel never emits in
/// practice but the parser doesn't reject), the LAST one wins as
/// `current`. All bracketed tokens still appear in `modes` with brackets
/// stripped.
fn parse_bracketed_modes(s: &str) -> (Vec<String>, Option<String>) {
    let mut modes = Vec::new();
    let mut current = None;
    for tok in s.split_whitespace() {
        if tok.starts_with('[') && tok.ends_with(']') && tok.len() >= 2 {
            let inner = &tok[1..tok.len() - 1];
            current = Some(inner.to_owned());
            modes.push(inner.to_owned());
        } else {
            modes.push(tok.to_owned());
        }
    }
    (modes, current)
}

/// Strip the optional `pci:` prefix that `/proc/acpi/wakeup`'s sysfs
/// column carries. Both `"pci:0000:00:01.1"` and `"0000:00:01.1"` are
/// accepted at the public boundary so callers don't have to massage the
/// string before passing it in.
#[allow(dead_code)]
fn strip_pci_prefix(addr: &str) -> &str {
    addr.strip_prefix("pci:").unwrap_or(addr)
}

#[allow(dead_code)]
fn pci_device_dir(root: &SysRoot, addr: &str) -> PathBuf {
    let bare = strip_pci_prefix(addr);
    root.join(format!("sys/bus/pci/devices/{bare}"))
}

/// Map the first six characters of a PCI class hex string (e.g.
/// `"0x0c0330"` → `"0x0c03"`) to a human-readable label.
///
/// Sourced verbatim from the Python tool's class_map (powercfg.py
/// lines 359-372). We keep the table here rather than in a shared
/// constants module because it's only used at this one call site.
#[allow(dead_code)]
fn pci_class_label(class_prefix: &str) -> Option<&'static str> {
    match class_prefix {
        "0x0c03" => Some("USB Controller"),
        "0x0c05" => Some("SMBus Controller"),
        "0x0200" => Some("Ethernet Controller"),
        "0x0280" => Some("Network Controller"),
        "0x0300" => Some("VGA Controller"),
        "0x0403" => Some("Audio Device"),
        "0x0108" => Some("NVMe Controller"),
        "0x0106" => Some("SATA Controller"),
        "0x0604" => Some("PCI Bridge"),
        "0x0600" => Some("Host Bridge"),
        "0x0580" => Some("Memory Controller"),
        _ => None,
    }
}

/// Map a PCI vendor hex string (e.g. `"0x1022"`) to a vendor name.
/// Mirrors the Python tool's vendor_map (powercfg.py lines 384-391).
#[allow(dead_code)]
fn pci_vendor_label(vendor: &str) -> Option<&'static str> {
    match vendor {
        "0x1022" => Some("AMD"),
        "0x10de" => Some("NVIDIA"),
        "0x8086" => Some("Intel"),
        "0x1002" => Some("AMD/ATI"),
        "0x14c3" => Some("MediaTek"),
        "0x10ec" => Some("Realtek"),
        _ => None,
    }
}

/// Get a human-readable description for a PCI device by sysfs address.
///
/// Reads `<root>/sys/bus/pci/devices/<addr>/{class,vendor}` and looks
/// the values up against the Python tool's class+vendor maps. Returns:
/// - `Ok(Some(desc))` when at least one of class/vendor matches.
/// - `Ok(None)` when the device exists but neither matches a known
///   table entry.
/// - `Err(SourceError::NotFound)` when the device directory itself is
///   absent (lets the caller surface "no such device" distinctly from
///   "device exists but unparseable").
/// - `Err(SourceError::Io)` for other read failures.
///
/// `addr` may carry the `pci:` prefix from `/proc/acpi/wakeup`
/// (`"pci:0000:00:01.1"`) or be bare (`"0000:00:01.1"`); both work.
///
/// Composition rule: if both vendor and class match, the result is
/// `"<Vendor> <Class>"` (vendor prepended). If only one matches, that
/// label is returned alone. Neither matching → `Ok(None)`.
// TODO(phase-2.2): wired by cmd::devicequery::run.
#[allow(dead_code)]
pub fn read_pci_device_description(
    root: &SysRoot,
    addr: &str,
) -> Result<Option<String>, SourceError> {
    let dir = pci_device_dir(root, addr);
    if !dir.exists() {
        return Err(SourceError::NotFound(format!(
            "pci device: {}",
            dir.display()
        )));
    }

    // Read class and vendor independently — either is allowed to be
    // missing and the descriptor falls back accordingly. The Python
    // tool uses a try/except around each read; we mirror that with
    // `.ok()` on the read so the absent-file case folds into "no
    // match" rather than aborting the descriptor.
    let class_label = match fs::read_to_string(dir.join("class")) {
        Ok(s) => {
            let class = s.trim();
            // class file content is like "0x0c0330"; we look up the
            // first six chars (the class+subclass).
            if class.len() >= 6 {
                pci_class_label(&class[..6])
            } else {
                None
            }
        }
        Err(_) => None,
    };

    let vendor_label = match fs::read_to_string(dir.join("vendor")) {
        Ok(s) => pci_vendor_label(s.trim()).map(|v| v.to_owned()),
        Err(_) => None,
    };

    let desc = match (vendor_label, class_label) {
        (Some(v), Some(c)) => Some(format!("{v} {c}")),
        (Some(v), None) => Some(v),
        (None, Some(c)) => Some(c.to_owned()),
        (None, None) => None,
    };
    Ok(desc)
}

/// Read wakeup statistics for a PCI device.
///
/// Reads all three of `<root>/sys/bus/pci/devices/<addr>/power/{
/// wakeup_count, wakeup_active_count, wakeup_last_time_ms}`. Matches
/// the Python tool's all-or-nothing behavior — the helper only returns
/// stats when every counter is present and parseable; otherwise the
/// caller gets `Err(Io)` (missing file) or `Err(Parse)` (non-numeric
/// content) and decides whether to degrade.
///
/// `addr` accepts the same `pci:`-prefixed and bare forms as
/// [`read_pci_device_description`].
// TODO(phase-2.2): wired by cmd::devicequery::run.
#[allow(dead_code)]
pub fn read_pci_wakeup_stats(root: &SysRoot, addr: &str) -> Result<WakeupStats, SourceError> {
    let dir = pci_device_dir(root, addr).join("power");
    let wakeup_count = read_u64_file(&dir, "wakeup_count")?;
    let wakeup_active_count = read_u64_file(&dir, "wakeup_active_count")?;
    let wakeup_last_time_ms = read_u64_file(&dir, "wakeup_last_time_ms")?;
    Ok(WakeupStats {
        wakeup_count,
        wakeup_active_count,
        wakeup_last_time_ms,
    })
}

#[allow(dead_code)]
fn read_u64_file(dir: &std::path::Path, name: &str) -> Result<u64, SourceError> {
    let path = dir.join(name);
    let content = fs::read_to_string(&path)?;
    let trimmed = content.trim();
    trimmed
        .parse::<u64>()
        .map_err(|e| SourceError::Parse(format!("{name}: {e} (input: {trimmed:?})")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_fixture(root: &SysRoot, rel: &str, content: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).expect("create dirs");
        fs::write(&path, content).expect("write fixture");
    }

    #[test]
    fn parse_bracketed_modes_current_first() {
        let (modes, current) = parse_bracketed_modes("[s2idle] deep");
        assert_eq!(modes, vec!["s2idle".to_string(), "deep".to_string()]);
        assert_eq!(current.as_deref(), Some("s2idle"));
    }

    #[test]
    fn parse_bracketed_modes_current_last() {
        let (modes, current) = parse_bracketed_modes("s2idle [deep]");
        assert_eq!(modes, vec!["s2idle".to_string(), "deep".to_string()]);
        assert_eq!(current.as_deref(), Some("deep"));
    }

    #[test]
    fn parse_bracketed_modes_no_bracket_marker() {
        let (modes, current) = parse_bracketed_modes("s2idle");
        assert_eq!(modes, vec!["s2idle".to_string()]);
        assert!(current.is_none());
    }

    #[test]
    fn parse_bracketed_modes_empty() {
        let (modes, current) = parse_bracketed_modes("");
        assert!(modes.is_empty());
        assert!(current.is_none());
    }

    #[test]
    fn parse_bracketed_modes_extra_whitespace() {
        // Leading/trailing whitespace + a tab in the middle should still
        // tokenize cleanly via split_whitespace.
        let (modes, current) = parse_bracketed_modes("  [s2idle]   deep  ");
        assert_eq!(modes, vec!["s2idle".to_string(), "deep".to_string()]);
        assert_eq!(current.as_deref(), Some("s2idle"));
    }

    #[test]
    fn parse_bracketed_modes_multiple_brackets_last_wins() {
        // The kernel never emits two bracketed tokens, but the parser
        // doesn't reject them — pin the last-write-wins rule explicitly
        // so a future contributor doesn't have to reverse-engineer it.
        let (modes, current) = parse_bracketed_modes("[s2idle] [deep]");
        assert_eq!(modes, vec!["s2idle".to_string(), "deep".to_string()]);
        assert_eq!(current.as_deref(), Some("deep"));
    }

    #[test]
    fn read_sleep_states_returns_tokens() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/state", "freeze mem disk\n");
        assert_eq!(
            read_sleep_states(&root).expect("read"),
            vec!["freeze", "mem", "disk"],
        );
    }

    #[test]
    fn read_sleep_states_empty_file_returns_empty_vec() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/state", "");
        assert!(read_sleep_states(&root).expect("read").is_empty());
    }

    #[test]
    fn read_sleep_states_missing_file_is_io_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let err = read_sleep_states(&root).expect_err("missing file should error");
        assert!(matches!(err, SourceError::Io(_)));
    }

    #[test]
    fn read_mem_sleep_modes_parses_bracketed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/mem_sleep", "[s2idle] deep\n");
        let (modes, current) = read_mem_sleep_modes(&root).expect("read");
        assert_eq!(modes, vec!["s2idle".to_string(), "deep".to_string()]);
        assert_eq!(current.as_deref(), Some("s2idle"));
    }

    #[test]
    fn read_disk_modes_parses_bracketed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/disk", "[platform] shutdown reboot\n");
        let (modes, current) = read_disk_modes(&root).expect("read");
        assert_eq!(modes, vec!["platform", "shutdown", "reboot"]);
        assert_eq!(current.as_deref(), Some("platform"));
    }

    #[test]
    fn read_image_size_bytes_parses_decimal() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/image_size", "4194304000\n");
        assert_eq!(read_image_size_bytes(&root).expect("read"), 4_194_304_000);
    }

    #[test]
    fn read_image_size_bytes_missing_is_io_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let err = read_image_size_bytes(&root).expect_err("missing file should error");
        assert!(matches!(err, SourceError::Io(_)));
    }

    #[test]
    fn read_image_size_bytes_non_numeric_is_parse_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/power/image_size", "not-a-number\n");
        let err = read_image_size_bytes(&root).expect_err("non-numeric should error");
        assert!(matches!(err, SourceError::Parse(_)));
    }

    /// Helper for the PCI tests: lay down `class`, `vendor`, `device`
    /// under `sys/bus/pci/devices/<addr>/` and the three power counters
    /// under `power/`.
    fn write_pci_device(
        root: &SysRoot,
        addr: &str,
        class: &str,
        vendor: &str,
        stats: Option<(&str, &str, &str)>,
    ) {
        let base = format!("sys/bus/pci/devices/{addr}");
        write_fixture(root, &format!("{base}/class"), class);
        write_fixture(root, &format!("{base}/vendor"), vendor);
        if let Some((count, active, last_ms)) = stats {
            write_fixture(root, &format!("{base}/power/wakeup_count"), count);
            write_fixture(root, &format!("{base}/power/wakeup_active_count"), active);
            write_fixture(root, &format!("{base}/power/wakeup_last_time_ms"), last_ms);
        }
    }

    #[test]
    fn read_pci_device_description_known_vendor_and_class() {
        // AMD (0x1022) USB Controller (0x0c0330 → 0x0c03) — both the
        // vendor and class tables hit, so the descriptor prepends the
        // vendor.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_pci_device(&root, "0000:00:01.1", "0x0c0330\n", "0x1022\n", None);
        let desc = read_pci_device_description(&root, "0000:00:01.1").expect("read");
        assert_eq!(desc.as_deref(), Some("AMD USB Controller"));
    }

    #[test]
    fn read_pci_device_description_vendor_only() {
        // Vendor matches (NVIDIA = 0x10de) but class is unknown — the
        // descriptor is just the vendor.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_pci_device(&root, "0000:01:00.0", "0x999999\n", "0x10de\n", None);
        let desc = read_pci_device_description(&root, "0000:01:00.0").expect("read");
        assert_eq!(desc.as_deref(), Some("NVIDIA"));
    }

    #[test]
    fn read_pci_device_description_class_only() {
        // Class matches (USB Controller) but vendor is unknown — the
        // descriptor is just the class label.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_pci_device(&root, "0000:02:00.0", "0x0c0330\n", "0xdeadbe\n", None);
        let desc = read_pci_device_description(&root, "0000:02:00.0").expect("read");
        assert_eq!(desc.as_deref(), Some("USB Controller"));
    }

    #[test]
    fn read_pci_device_description_neither_known_returns_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_pci_device(&root, "0000:03:00.0", "0x999999\n", "0xdeadbe\n", None);
        let desc = read_pci_device_description(&root, "0000:03:00.0").expect("read");
        assert!(
            desc.is_none(),
            "neither table hit, expected None, got {desc:?}"
        );
    }

    #[test]
    fn read_pci_device_description_missing_dir_is_not_found() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let err = read_pci_device_description(&root, "0000:99:99.9")
            .expect_err("missing device dir should error");
        assert!(
            matches!(err, SourceError::NotFound(_)),
            "expected NotFound, got {err:?}",
        );
    }

    #[test]
    fn read_pci_device_description_accepts_pci_prefix() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_pci_device(&root, "0000:00:01.1", "0x0c0330\n", "0x1022\n", None);
        // pci:-prefixed (as it appears in /proc/acpi/wakeup) should
        // resolve to the same descriptor as the bare form.
        let with_prefix =
            read_pci_device_description(&root, "pci:0000:00:01.1").expect("read with prefix");
        let bare = read_pci_device_description(&root, "0000:00:01.1").expect("read bare");
        assert_eq!(with_prefix, bare);
        assert_eq!(with_prefix.as_deref(), Some("AMD USB Controller"));
    }

    #[test]
    fn read_pci_wakeup_stats_typical() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_pci_device(
            &root,
            "0000:00:01.1",
            "0x060400\n",
            "0x1022\n",
            Some(("5\n", "5\n", "12345\n")),
        );
        let stats = read_pci_wakeup_stats(&root, "0000:00:01.1").expect("read stats");
        assert_eq!(
            stats,
            WakeupStats {
                wakeup_count: 5,
                wakeup_active_count: 5,
                wakeup_last_time_ms: 12345,
            }
        );
    }

    #[test]
    fn read_pci_wakeup_stats_pci_prefix_works() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_pci_device(
            &root,
            "0000:00:01.1",
            "0x060400\n",
            "0x1022\n",
            Some(("1\n", "0\n", "999\n")),
        );
        let stats = read_pci_wakeup_stats(&root, "pci:0000:00:01.1").expect("read");
        assert_eq!(stats.wakeup_count, 1);
        assert_eq!(stats.wakeup_last_time_ms, 999);
    }

    #[test]
    fn read_pci_wakeup_stats_missing_one_file_is_io_error() {
        // Lay down two of the three counter files; the third missing
        // file should surface as a typed Io failure rather than a
        // silently zero stat.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let base = "sys/bus/pci/devices/0000:00:01.1";
        write_fixture(&root, &format!("{base}/class"), "0x060400\n");
        write_fixture(&root, &format!("{base}/vendor"), "0x1022\n");
        write_fixture(&root, &format!("{base}/power/wakeup_count"), "5\n");
        write_fixture(&root, &format!("{base}/power/wakeup_active_count"), "5\n");
        // wakeup_last_time_ms intentionally omitted.
        let err =
            read_pci_wakeup_stats(&root, "0000:00:01.1").expect_err("missing file should error");
        assert!(matches!(err, SourceError::Io(_)));
    }

    #[test]
    fn read_pci_wakeup_stats_non_numeric_is_parse_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_pci_device(
            &root,
            "0000:00:01.1",
            "0x060400\n",
            "0x1022\n",
            Some(("oops\n", "0\n", "0\n")),
        );
        let err =
            read_pci_wakeup_stats(&root, "0000:00:01.1").expect_err("non-numeric should error");
        assert!(matches!(err, SourceError::Parse(_)));
    }
}
