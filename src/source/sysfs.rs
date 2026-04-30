//! sysfs readers for `/sys`.
//!
//! Functions take a `&SysRoot` so integration tests can point at fixture
//! trees (`POWERCFG_SYSROOT=tests/fixtures/sys-typical`). Every reader
//! returns `Result<T, SourceError>`; the command layer decides whether a
//! missing file or unreadable permission should render as "no data" or as
//! a hard error.

use std::fs;
use std::path::PathBuf;

use crate::model::devicequery::{UsbWakeDevice, WakeupStats};
use crate::model::energy::{CpuFreqInfo, PowerSupply, ThermalReading, ThrottleStatus};
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
fn strip_pci_prefix(addr: &str) -> &str {
    addr.strip_prefix("pci:").unwrap_or(addr)
}

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

fn read_u64_file(dir: &std::path::Path, name: &str) -> Result<u64, SourceError> {
    let path = dir.join(name);
    let content = fs::read_to_string(&path)?;
    let trimmed = content.trim();
    trimmed
        .parse::<u64>()
        .map_err(|e| SourceError::Parse(format!("{name}: {e} (input: {trimmed:?})")))
}

/// Walk `<root>/sys/bus/usb/devices/` and collect USB devices that have
/// `power/wakeup == "enabled"`.
///
/// For each direntry that is a directory and has a `power/wakeup` file:
/// - Skip unless trimmed content is `"enabled"`.
/// - Read `manufacturer` and `product` if present (each may be missing).
/// - Display name = `format!("{manufacturer} {product}").trim()`. If
///   that ends up empty, fall back to the directory name (e.g. `"1-2"`).
///
/// Per-device read errors (permission denied, missing files) are
/// silently skipped with a `tracing::debug!` so they surface under
/// `RUST_LOG=debug` without breaking the report. Only failure to
/// enumerate the parent `usb/devices` directory raises
/// `Err(SourceError::Io)`.
///
/// If the parent directory doesn't exist (headless / no USB bus), this
/// returns `Ok(vec![])` — matches Python's `if usb_path.exists():`
/// guard at line 148, where a missing path is "no USB devices" rather
/// than an error.
pub fn read_usb_wakeup_devices(root: &SysRoot) -> Result<Vec<UsbWakeDevice>, SourceError> {
    let parent = root.join("sys/bus/usb/devices");
    if !parent.exists() {
        return Ok(vec![]);
    }
    let mut devices = Vec::new();
    let entries = fs::read_dir(&parent)?;
    // Sort by directory name for deterministic output (read_dir order
    // is filesystem-dependent and would make snapshots flaky).
    let mut dir_names: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            (name, path)
        })
        .collect();
    dir_names.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, path) in dir_names {
        if !path.is_dir() {
            continue;
        }
        let wakeup_file = path.join("power/wakeup");
        let status = match fs::read_to_string(&wakeup_file) {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!("usb device {name}: {e}");
                continue;
            }
        };
        if status.trim() != "enabled" {
            continue;
        }

        let manufacturer = fs::read_to_string(path.join("manufacturer"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        let product = fs::read_to_string(path.join("product"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        let display = format!("{manufacturer} {product}").trim().to_owned();
        let display = if display.is_empty() {
            name.clone()
        } else {
            display
        };
        devices.push(UsbWakeDevice {
            device: name,
            name: display,
        });
    }
    Ok(devices)
}

/// Walk `<root>/sys/class/power_supply/` and collect one [`PowerSupply`]
/// per direntry.
///
/// Reads the per-supply files independently — any single missing/unreadable
/// file folds into the corresponding `Option` field as `None` rather than
/// aborting the row. The Python tool does the same with a per-supply
/// `try/except: pass`.
///
/// `capacity_pct` parses the file as `u8`; values outside `0..=100` (or
/// non-numeric content) yield `None` for that field. `power_uw` parses as
/// `u64` with the same skip-on-error behavior.
///
/// If the parent `/sys/class/power_supply/` directory does not exist
/// (headless desktop), returns `Ok(vec![])` — matches Python line 774's
/// `if ps_path.exists():` guard. If the directory exists but `read_dir`
/// fails (permission denied, transient errors), the error propagates.
pub fn read_power_supplies(root: &SysRoot) -> Result<Vec<PowerSupply>, SourceError> {
    let parent = root.join("sys/class/power_supply");
    if !parent.exists() {
        return Ok(vec![]);
    }
    let entries = fs::read_dir(&parent)?;
    // Sort case-insensitively by directory name for deterministic output
    // across runs and across machines that mix `BAT0`/`bat0` casing.
    let mut dir_names: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    dir_names.sort_by_key(|a| a.0.to_ascii_lowercase());

    let mut supplies = Vec::with_capacity(dir_names.len());
    for (name, path) in dir_names {
        let kind = read_trimmed(&path, "type");
        let status = read_trimmed(&path, "status");
        let capacity_pct = read_trimmed(&path, "capacity").and_then(|s| s.parse::<u8>().ok());
        let level = read_trimmed(&path, "capacity_level");
        let power_uw = read_trimmed(&path, "power_now").and_then(|s| s.parse::<u64>().ok());
        supplies.push(PowerSupply {
            name,
            kind,
            status,
            capacity_pct,
            level,
            power_uw,
        });
    }
    Ok(supplies)
}

/// Read CPU frequency / governor / EPP info from `cpu0/cpufreq` and count
/// CPUs by counting `cpuN` direntries under `/sys/devices/system/cpu/`.
///
/// All field reads are independent — a missing `scaling_cur_freq` doesn't
/// suppress `scaling_driver`. Non-numeric content in the freq files folds
/// into `None` for that field rather than erroring.
///
/// Returns `Ok(CpuFreqInfo::default())` when the parent
/// `/sys/devices/system/cpu/` directory itself doesn't exist (some test
/// fixtures don't carry it). When that parent exists but
/// `cpu0/cpufreq/` doesn't (e.g. kernels built without cpufreq), returns
/// a `CpuFreqInfo` with every field None except `cpu_count` — the count
/// is still meaningful.
///
/// `cpu_count` matches the Python tool's `d.name.startswith("cpu") and
/// d.name[3:].isdigit()` filter (regex-free).
pub fn read_cpu_freq_info(root: &SysRoot) -> Result<CpuFreqInfo, SourceError> {
    let cpu_root = root.join("sys/devices/system/cpu");
    if !cpu_root.exists() {
        return Ok(CpuFreqInfo::default());
    }

    let mut info = CpuFreqInfo::default();

    // Per-CPU fields from cpu0/cpufreq. Missing-dir is a no-op (info stays
    // empty); a real I/O failure during read is folded into None per-field.
    let cpu0_freq = cpu_root.join("cpu0/cpufreq");
    if cpu0_freq.exists() {
        info.driver = read_trimmed(&cpu0_freq, "scaling_driver");
        info.governor = read_trimmed(&cpu0_freq, "scaling_governor");
        info.cur_freq_khz =
            read_trimmed(&cpu0_freq, "scaling_cur_freq").and_then(|s| s.parse::<u64>().ok());
        info.min_freq_khz =
            read_trimmed(&cpu0_freq, "scaling_min_freq").and_then(|s| s.parse::<u64>().ok());
        info.max_freq_khz =
            read_trimmed(&cpu0_freq, "scaling_max_freq").and_then(|s| s.parse::<u64>().ok());
        info.epp = read_trimmed(&cpu0_freq, "energy_performance_preference");
        info.epp_available = read_trimmed(&cpu0_freq, "energy_performance_available_preferences");
    }

    // Count cpuN direntries (regex-free).
    info.cpu_count = match fs::read_dir(&cpu_root) {
        Ok(iter) => iter
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.starts_with("cpu")
                    && name.len() > 3
                    && name[3..].chars().all(|c| c.is_ascii_digit())
            })
            .count(),
        Err(_) => 0,
    };

    Ok(info)
}

/// Walk `<root>/sys/class/hwmon/` and collect [`ThermalReading`]s for the
/// CPU-thermal chips Python's `get_thermal_info` knows about: `k10temp`,
/// `coretemp`, `zenpower`. Other hwmon entries (NVMe, fans, GPU sensors)
/// are silently skipped so the section stays focused on CPU thermals.
///
/// For each matching chip, every `tempN_input` file is read as
/// millidegrees Celsius, divided by 1000 for the float result, and paired
/// with the chip's `tempN_label` if present. When the label file is
/// absent (which happens in practice for some inputs on some kernels),
/// the label falls back to the literal `"CPU"` — matches Python line 881.
///
/// Per-chip and per-temp read errors are tolerated (logged at debug,
/// reading continues). Missing parent → `Ok(vec![])` (Python line 868).
pub fn read_thermal_info(root: &SysRoot) -> Result<Vec<ThermalReading>, SourceError> {
    let parent = root.join("sys/class/hwmon");
    if !parent.exists() {
        return Ok(vec![]);
    }
    let entries = fs::read_dir(&parent)?;
    // Sort by hwmonN name for deterministic output.
    let mut chips: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    chips.sort_by(|a, b| a.0.cmp(&b.0));

    let mut readings = Vec::new();
    for (chip_name, chip_path) in chips {
        if !chip_path.is_dir() {
            continue;
        }
        let name = match read_trimmed(&chip_path, "name") {
            Some(n) => n,
            None => {
                tracing::debug!("hwmon {chip_name}: missing name file, skipping");
                continue;
            }
        };
        if !matches!(name.as_str(), "k10temp" | "coretemp" | "zenpower") {
            continue;
        }

        // Find every tempN_input. read_dir order is filesystem-dependent;
        // sort by filename so callers see a stable ordering.
        let mut inputs: Vec<(String, PathBuf)> = match fs::read_dir(&chip_path) {
            Ok(iter) => iter
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let fname = e.file_name().to_string_lossy().into_owned();
                    if fname.starts_with("temp") && fname.ends_with("_input") {
                        Some((fname, e.path()))
                    } else {
                        None
                    }
                })
                .collect(),
            Err(e) => {
                tracing::debug!("hwmon {chip_name} read_dir: {e}");
                continue;
            }
        };
        // Sort by the numeric suffix so temp2 < temp10 — lexicographic
        // ordering would put temp10 before temp2, which is fine for
        // 1-9 sensors but breaks on real k10temp/zenpower hardware that
        // exposes 10+ readings. Unparseable suffixes sort to the end.
        inputs.sort_by_key(|(fname, _)| {
            fname
                .trim_start_matches("temp")
                .trim_end_matches("_input")
                .parse::<u32>()
                .unwrap_or(u32::MAX)
        });

        for (fname, input_path) in inputs {
            // "temp1_input" → "temp1".
            let temp_num = match fname.strip_suffix("_input") {
                Some(n) => n,
                None => continue, // shouldn't happen given the filter above
            };
            let mc_text = match fs::read_to_string(&input_path) {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("hwmon {chip_name}/{fname}: {e}");
                    continue;
                }
            };
            let mc: i64 = match mc_text.trim().parse() {
                Ok(n) => n,
                Err(e) => {
                    tracing::debug!("hwmon {chip_name}/{fname}: non-numeric ({e}), skipping",);
                    continue;
                }
            };
            let label = read_trimmed(&chip_path, &format!("{temp_num}_label"))
                .unwrap_or_else(|| "CPU".to_owned());
            readings.push(ThermalReading {
                label,
                temp_c: mc as f64 / 1000.0,
                source: name.clone(),
            });
        }
    }
    Ok(readings)
}

/// Sum the integer contents of every
/// `<root>/sys/devices/system/cpu/cpuN/thermal_throttle/package_throttle_count`
/// file into a single counter. Per-CPU read or parse failures are
/// skipped silently (logged at debug); they don't poison the running
/// total. A missing parent directory contributes zero.
///
/// The function returns `Ok(ThrottleStatus { … })` even when every
/// individual read failed — `Default` is the zero state and we don't
/// want a healthy box without these counters to look like an error.
///
/// Diverges from Python: the Python tool also walked
/// `thermal_zone*/mode` looking for a `"disabled"` value to set a
/// `throttled` boolean. That walk is dropped here because the kernel
/// ABI for `mode` is "thermal-zone administratively enabled?" — not
/// "is the CPU currently being throttled?" — so the boolean reported
/// the wrong thing. The historical `throttle_count` is the only
/// meaningful current/historical signal at this layer, so the printer
/// in 2.4 only renders that.
pub fn read_throttle_status(root: &SysRoot) -> Result<ThrottleStatus, SourceError> {
    let mut status = ThrottleStatus::default();

    let cpu_root = root.join("sys/devices/system/cpu");
    if cpu_root.exists() {
        if let Ok(iter) = fs::read_dir(&cpu_root) {
            for entry in iter.filter_map(|e| e.ok()) {
                let fname = entry.file_name();
                let fname = fname.to_string_lossy();
                if !(fname.starts_with("cpu")
                    && fname.len() > 3
                    && fname[3..].chars().all(|c| c.is_ascii_digit()))
                {
                    continue;
                }
                let path = entry.path().join("thermal_throttle/package_throttle_count");
                let text = match fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::debug!("{fname} package_throttle_count: {e}");
                        continue;
                    }
                };
                match text.trim().parse::<u64>() {
                    Ok(n) => status.throttle_count = status.throttle_count.saturating_add(n),
                    Err(e) => {
                        tracing::debug!("{fname} package_throttle_count parse: {e}");
                    }
                }
            }
        }
    }

    Ok(status)
}

/// Helper: read `dir/name` and return the trimmed contents as `Some` on
/// success, `None` on any I/O failure. Used by the per-field readers in
/// the new sources where a missing file is "no value here" rather than
/// an error.
fn read_trimmed(dir: &std::path::Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name))
        .ok()
        .map(|s| s.trim().to_owned())
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

    #[test]
    fn read_pci_descriptor_typical_fixture_tree() {
        // Lock the committed sys-typical PCI fixture against the reader.
        // Drift between fixture layout and parser would otherwise only
        // surface in 2.2's integration tests.
        let root = SysRoot::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sys-typical",
        ));
        // Fixture device: class 0x060400 (PCI Bridge), vendor 0x1022 (AMD).
        let desc = read_pci_device_description(&root, "0000:00:01.1")
            .expect("descriptor read against fixture");
        assert_eq!(desc.as_deref(), Some("AMD PCI Bridge"));
    }

    #[test]
    fn read_pci_wakeup_stats_typical_fixture_tree() {
        let root = SysRoot::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sys-typical",
        ));
        let stats = read_pci_wakeup_stats(&root, "0000:00:01.1").expect("stats from fixture");
        assert_eq!(
            stats,
            WakeupStats {
                wakeup_count: 5,
                wakeup_active_count: 5,
                wakeup_last_time_ms: 12345,
            }
        );
    }

    /// Helper: lay down a USB device dir with the given files. Each of
    /// `wakeup`, `manufacturer`, `product` is optional; pass `None` to
    /// omit the file entirely (exercising the "missing file" path).
    fn write_usb_device(
        root: &SysRoot,
        name: &str,
        wakeup: Option<&str>,
        manufacturer: Option<&str>,
        product: Option<&str>,
    ) {
        let base = format!("sys/bus/usb/devices/{name}");
        // Always create the directory so it shows up in read_dir even
        // when no files are written.
        fs::create_dir_all(root.join(&base)).expect("create usb dir");
        if let Some(w) = wakeup {
            write_fixture(root, &format!("{base}/power/wakeup"), w);
        }
        if let Some(m) = manufacturer {
            write_fixture(root, &format!("{base}/manufacturer"), m);
        }
        if let Some(p) = product {
            write_fixture(root, &format!("{base}/product"), p);
        }
    }

    #[test]
    fn read_usb_wakeup_devices_manufacturer_and_product_join() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_usb_device(
            &root,
            "1-2",
            Some("enabled\n"),
            Some("Logitech\n"),
            Some("USB Receiver\n"),
        );
        let devices = read_usb_wakeup_devices(&root).expect("read");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device, "1-2");
        assert_eq!(devices[0].name, "Logitech USB Receiver");
    }

    #[test]
    fn read_usb_wakeup_devices_missing_manufacturer_uses_product_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_usb_device(
            &root,
            "1-2",
            Some("enabled\n"),
            None,
            Some("USB Receiver\n"),
        );
        let devices = read_usb_wakeup_devices(&root).expect("read");
        assert_eq!(devices.len(), 1);
        // format!("{} {}", "", "USB Receiver").trim() == "USB Receiver"
        assert_eq!(devices[0].name, "USB Receiver");
    }

    #[test]
    fn read_usb_wakeup_devices_missing_both_falls_back_to_dir_name() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_usb_device(&root, "2-1", Some("enabled\n"), None, None);
        let devices = read_usb_wakeup_devices(&root).expect("read");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device, "2-1");
        assert_eq!(devices[0].name, "2-1");
    }

    #[test]
    fn read_usb_wakeup_devices_disabled_excluded() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_usb_device(&root, "1-3", Some("disabled\n"), None, None);
        let devices = read_usb_wakeup_devices(&root).expect("read");
        assert!(devices.is_empty());
    }

    #[test]
    fn read_usb_wakeup_devices_missing_wakeup_file_excluded() {
        // Directory exists but power/wakeup is absent — Python's
        // `if wakeup_file.exists():` guard skips it; we match that.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_usb_device(&root, "1-4", None, Some("Acme\n"), Some("Widget\n"));
        let devices = read_usb_wakeup_devices(&root).expect("read");
        assert!(devices.is_empty());
    }

    #[test]
    fn read_usb_wakeup_devices_missing_parent_returns_empty() {
        // Headless / no USB tree → Ok(vec![]) rather than an error.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let devices = read_usb_wakeup_devices(&root).expect("read");
        assert!(devices.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn read_usb_wakeup_devices_follows_symlinks() {
        // Real /sys/bus/usb/devices/ has a mix of real entries and
        // symlinks (usb1, usb2, 1-0:1.0, ...) pointing to other USB
        // device dirs. Path::is_dir() follows symlinks, so the walker
        // should pick up symlinked entries the same as real ones.
        // Fixtures use real dirs only — this test covers the symlink
        // branch explicitly so a future regression doesn't ghost it.
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());

        // Real device at 1-2.
        let usb_dir = tmp.path().join("sys/bus/usb/devices");
        std::fs::create_dir_all(&usb_dir).expect("mkdir");
        let real = usb_dir.join("1-2");
        std::fs::create_dir_all(real.join("power")).expect("mkdir");
        std::fs::write(real.join("power/wakeup"), "enabled\n").expect("wakeup");
        std::fs::write(real.join("manufacturer"), "Logitech\n").expect("manuf");
        std::fs::write(real.join("product"), "USB Receiver\n").expect("prod");

        // Symlink usb1 -> 1-2 (mimicking a kernel-emitted alias).
        symlink("1-2", usb_dir.join("usb1")).expect("symlink");

        let mut devices = read_usb_wakeup_devices(&root).expect("read");
        devices.sort_by(|a, b| a.device.cmp(&b.device));
        assert_eq!(devices.len(), 2, "real entry plus symlink alias");
        assert_eq!(devices[0].device, "1-2");
        assert_eq!(devices[1].device, "usb1");
        // Both pick up the same metadata since the symlink resolves
        // to the same files.
        assert_eq!(devices[0].name, "Logitech USB Receiver");
        assert_eq!(devices[1].name, "Logitech USB Receiver");
    }

    // ----------------------------------------------------------------
    // read_power_supplies
    // ----------------------------------------------------------------

    #[test]
    fn read_power_supplies_typical_ac_and_battery() {
        // Two supplies: AC adapter (no capacity) and battery (full data).
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/class/power_supply/AC/type", "Mains\n");
        write_fixture(&root, "sys/class/power_supply/AC/status", "Discharging\n");
        write_fixture(&root, "sys/class/power_supply/BAT0/type", "Battery\n");
        write_fixture(&root, "sys/class/power_supply/BAT0/status", "Discharging\n");
        write_fixture(&root, "sys/class/power_supply/BAT0/capacity", "87\n");
        write_fixture(
            &root,
            "sys/class/power_supply/BAT0/capacity_level",
            "Normal\n",
        );

        let supplies = read_power_supplies(&root).expect("read");
        // Sorted alphabetically: AC, BAT0.
        assert_eq!(supplies.len(), 2);
        assert_eq!(supplies[0].name, "AC");
        assert_eq!(supplies[0].kind.as_deref(), Some("Mains"));
        assert_eq!(supplies[0].status.as_deref(), Some("Discharging"));
        assert_eq!(supplies[0].capacity_pct, None);
        assert_eq!(supplies[0].level, None);
        assert_eq!(supplies[0].power_uw, None);

        assert_eq!(supplies[1].name, "BAT0");
        assert_eq!(supplies[1].kind.as_deref(), Some("Battery"));
        assert_eq!(supplies[1].status.as_deref(), Some("Discharging"));
        assert_eq!(supplies[1].capacity_pct, Some(87));
        assert_eq!(supplies[1].level.as_deref(), Some("Normal"));
        assert_eq!(supplies[1].power_uw, None);
    }

    #[test]
    fn read_power_supplies_battery_with_power_now() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/class/power_supply/BAT0/type", "Battery\n");
        write_fixture(&root, "sys/class/power_supply/BAT0/power_now", "12500000\n");

        let supplies = read_power_supplies(&root).expect("read");
        assert_eq!(supplies.len(), 1);
        assert_eq!(supplies[0].power_uw, Some(12_500_000));
    }

    #[test]
    fn read_power_supplies_missing_parent_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let supplies = read_power_supplies(&root).expect("read");
        assert!(supplies.is_empty());
    }

    #[test]
    fn read_power_supplies_empty_parent_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        // Lay down the parent dir but no children — read_dir succeeds and
        // yields nothing.
        fs::create_dir_all(root.join("sys/class/power_supply")).expect("mkdir");
        let supplies = read_power_supplies(&root).expect("read");
        assert!(supplies.is_empty());
    }

    #[test]
    fn read_power_supplies_capacity_out_of_range_drops_field() {
        // Capacity of 200 doesn't fit u8 ≤ 100 conceptually, but u8::parse
        // accepts up to 255. The value 257 is out of u8 range and parse()
        // returns Err — we want None on the row, not a panic.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/class/power_supply/BAT0/type", "Battery\n");
        write_fixture(&root, "sys/class/power_supply/BAT0/capacity", "257\n");

        let supplies = read_power_supplies(&root).expect("read");
        assert_eq!(supplies.len(), 1);
        assert_eq!(supplies[0].capacity_pct, None);
    }

    #[test]
    fn read_power_supplies_garbage_power_now_drops_field() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/class/power_supply/BAT0/type", "Battery\n");
        write_fixture(
            &root,
            "sys/class/power_supply/BAT0/power_now",
            "not-a-number\n",
        );

        let supplies = read_power_supplies(&root).expect("read");
        assert_eq!(supplies.len(), 1);
        assert_eq!(supplies[0].power_uw, None);
    }

    // ----------------------------------------------------------------
    // read_cpu_freq_info
    // ----------------------------------------------------------------

    /// Helper for the cpufreq tests: lay down a populated `cpu0/cpufreq`
    /// tree under the given root.
    fn write_full_cpufreq(root: &SysRoot) {
        let base = "sys/devices/system/cpu/cpu0/cpufreq";
        write_fixture(root, &format!("{base}/scaling_driver"), "amd-pstate-epp\n");
        write_fixture(root, &format!("{base}/scaling_governor"), "powersave\n");
        write_fixture(root, &format!("{base}/scaling_cur_freq"), "3400000\n");
        write_fixture(root, &format!("{base}/scaling_min_freq"), "400000\n");
        write_fixture(root, &format!("{base}/scaling_max_freq"), "4800000\n");
        write_fixture(
            root,
            &format!("{base}/energy_performance_preference"),
            "balance_performance\n",
        );
        write_fixture(
            root,
            &format!("{base}/energy_performance_available_preferences"),
            "default performance balance_performance balance_power power\n",
        );
    }

    #[test]
    fn read_cpu_freq_info_full_fixture() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_full_cpufreq(&root);
        // 16 cpuN dirs (cpu0..cpu15). cpu0 already exists from the cpufreq
        // tree above.
        for n in 1..16 {
            fs::create_dir_all(root.join(format!("sys/devices/system/cpu/cpu{n}")))
                .expect("mkdir cpuN");
        }

        let info = read_cpu_freq_info(&root).expect("read");
        assert_eq!(info.driver.as_deref(), Some("amd-pstate-epp"));
        assert_eq!(info.governor.as_deref(), Some("powersave"));
        assert_eq!(info.cur_freq_khz, Some(3_400_000));
        assert_eq!(info.min_freq_khz, Some(400_000));
        assert_eq!(info.max_freq_khz, Some(4_800_000));
        assert_eq!(info.epp.as_deref(), Some("balance_performance"));
        assert_eq!(
            info.epp_available.as_deref(),
            Some("default performance balance_performance balance_power power"),
        );
        assert_eq!(info.cpu_count, 16);
    }

    #[test]
    fn read_cpu_freq_info_only_driver_present() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(
            &root,
            "sys/devices/system/cpu/cpu0/cpufreq/scaling_driver",
            "intel_pstate\n",
        );

        let info = read_cpu_freq_info(&root).expect("read");
        assert_eq!(info.driver.as_deref(), Some("intel_pstate"));
        assert_eq!(info.governor, None);
        assert_eq!(info.cur_freq_khz, None);
        assert_eq!(info.epp, None);
        assert_eq!(info.cpu_count, 1, "cpu0 dir alone");
    }

    #[test]
    fn read_cpu_freq_info_no_cpufreq_dir_but_cpus_exist() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        // 4 cpuN dirs but no cpufreq subdir.
        for n in 0..4 {
            fs::create_dir_all(root.join(format!("sys/devices/system/cpu/cpu{n}")))
                .expect("mkdir cpuN");
        }
        let info = read_cpu_freq_info(&root).expect("read");
        assert_eq!(info.driver, None);
        assert_eq!(info.governor, None);
        assert_eq!(info.cpu_count, 4);
    }

    #[test]
    fn read_cpu_freq_info_missing_parent_is_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let info = read_cpu_freq_info(&root).expect("read");
        assert_eq!(info.driver, None);
        assert_eq!(info.cpu_count, 0);
    }

    #[test]
    fn read_cpu_freq_info_garbage_freq_is_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(
            &root,
            "sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq",
            "not-a-number\n",
        );
        let info = read_cpu_freq_info(&root).expect("read");
        // The driver is missing, the bad freq folds to None — but cur_freq
        // doesn't flip the function to Err.
        assert_eq!(info.cur_freq_khz, None);
    }

    #[test]
    fn read_cpu_freq_info_ignores_non_cpu_dirs() {
        // /sys/devices/system/cpu has a bunch of non-cpuN siblings on a
        // real kernel: cpufreq, cpuidle, hotplug, isolated, kernel_max,
        // present, online, possible, …
        // The cpu_count walk should ignore them (only `cpu` + digits count).
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        for n in 0..2 {
            fs::create_dir_all(root.join(format!("sys/devices/system/cpu/cpu{n}")))
                .expect("mkdir cpuN");
        }
        // Distractors:
        fs::create_dir_all(root.join("sys/devices/system/cpu/cpufreq")).expect("mkdir");
        fs::create_dir_all(root.join("sys/devices/system/cpu/cpuidle")).expect("mkdir");
        fs::write(root.join("sys/devices/system/cpu/online"), "0-1\n").expect("file");
        fs::write(root.join("sys/devices/system/cpu/cpubogus"), "x\n").expect("file");

        let info = read_cpu_freq_info(&root).expect("read");
        assert_eq!(info.cpu_count, 2);
    }

    // ----------------------------------------------------------------
    // read_thermal_info
    // ----------------------------------------------------------------

    #[test]
    fn read_thermal_info_k10temp_two_inputs_label_fallback() {
        // hwmon0: k10temp with temp1_input + temp1_label, and temp2_input
        // (no label — exercises the "CPU" fallback).
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let base = "sys/class/hwmon/hwmon0";
        write_fixture(&root, &format!("{base}/name"), "k10temp\n");
        write_fixture(&root, &format!("{base}/temp1_input"), "52400\n");
        write_fixture(&root, &format!("{base}/temp1_label"), "Tctl\n");
        write_fixture(&root, &format!("{base}/temp2_input"), "50100\n");

        let readings = read_thermal_info(&root).expect("read");
        assert_eq!(readings.len(), 2);
        assert_eq!(readings[0].label, "Tctl");
        assert!((readings[0].temp_c - 52.4).abs() < 1e-9);
        assert_eq!(readings[0].source, "k10temp");
        // temp2 falls back to "CPU".
        assert_eq!(readings[1].label, "CPU");
        assert!((readings[1].temp_c - 50.1).abs() < 1e-9);
    }

    #[test]
    fn read_thermal_info_filters_unrelated_chips() {
        // hwmon0: nvme0 (skipped), hwmon1: k10temp (kept).
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/class/hwmon/hwmon0/name", "nvme0\n");
        write_fixture(&root, "sys/class/hwmon/hwmon0/temp1_input", "35000\n");
        write_fixture(&root, "sys/class/hwmon/hwmon1/name", "k10temp\n");
        write_fixture(&root, "sys/class/hwmon/hwmon1/temp1_input", "55000\n");
        write_fixture(&root, "sys/class/hwmon/hwmon1/temp1_label", "Tctl\n");

        let readings = read_thermal_info(&root).expect("read");
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].source, "k10temp");
        assert_eq!(readings[0].label, "Tctl");
    }

    #[test]
    fn read_thermal_info_missing_parent_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let readings = read_thermal_info(&root).expect("read");
        assert!(readings.is_empty());
    }

    #[test]
    fn read_thermal_info_non_numeric_temp_skipped() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/class/hwmon/hwmon0/name", "k10temp\n");
        write_fixture(&root, "sys/class/hwmon/hwmon0/temp1_input", "garbage\n");
        write_fixture(&root, "sys/class/hwmon/hwmon0/temp2_input", "60000\n");

        let readings = read_thermal_info(&root).expect("read");
        // temp1 skipped (non-numeric), temp2 kept.
        assert_eq!(readings.len(), 1);
        assert!((readings[0].temp_c - 60.0).abs() < 1e-9);
    }

    #[test]
    fn read_thermal_info_chip_with_no_name_skipped() {
        // hwmon0 has no `name` file at all — Python's `if name_file.exists()`
        // skips it; we match that behavior.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/class/hwmon/hwmon0/temp1_input", "50000\n");

        let readings = read_thermal_info(&root).expect("read");
        assert!(readings.is_empty());
    }

    #[test]
    fn read_thermal_info_coretemp_and_zenpower_match() {
        // Both are accepted alongside k10temp.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/class/hwmon/hwmon0/name", "coretemp\n");
        write_fixture(&root, "sys/class/hwmon/hwmon0/temp1_input", "45000\n");
        write_fixture(&root, "sys/class/hwmon/hwmon1/name", "zenpower\n");
        write_fixture(&root, "sys/class/hwmon/hwmon1/temp1_input", "48000\n");

        let readings = read_thermal_info(&root).expect("read");
        assert_eq!(readings.len(), 2);
        assert_eq!(readings[0].source, "coretemp");
        assert_eq!(readings[1].source, "zenpower");
    }

    // ----------------------------------------------------------------
    // read_throttle_status
    // ----------------------------------------------------------------

    #[test]
    fn read_throttle_status_sums_per_cpu_counts() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(
            &root,
            "sys/devices/system/cpu/cpu0/thermal_throttle/package_throttle_count",
            "5\n",
        );
        write_fixture(
            &root,
            "sys/devices/system/cpu/cpu1/thermal_throttle/package_throttle_count",
            "5\n",
        );

        let status = read_throttle_status(&root).expect("read");
        assert_eq!(status.throttle_count, 10);
    }

    #[test]
    fn read_throttle_status_missing_parents_returns_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        let status = read_throttle_status(&root).expect("read");
        assert_eq!(status, ThrottleStatus::default());
    }

    #[test]
    fn read_throttle_status_partial_per_cpu_files() {
        // Two cpu dirs but only cpu0 has a readable count file — the sum
        // should be cpu0's contribution alone.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(
            &root,
            "sys/devices/system/cpu/cpu0/thermal_throttle/package_throttle_count",
            "7\n",
        );
        // cpu1 exists but with no throttle file.
        fs::create_dir_all(root.join("sys/devices/system/cpu/cpu1")).expect("mkdir");

        let status = read_throttle_status(&root).expect("read");
        assert_eq!(status.throttle_count, 7);
    }

    #[test]
    fn read_throttle_status_garbage_count_skipped() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(
            &root,
            "sys/devices/system/cpu/cpu0/thermal_throttle/package_throttle_count",
            "not-a-number\n",
        );
        write_fixture(
            &root,
            "sys/devices/system/cpu/cpu1/thermal_throttle/package_throttle_count",
            "3\n",
        );
        let status = read_throttle_status(&root).expect("read");
        assert_eq!(status.throttle_count, 3);
    }

    #[test]
    fn read_throttle_status_ignores_thermal_zone_files() {
        // The Python tool walked thermal_zone*/mode looking for "disabled"
        // values to set a `throttled` flag — that walk is gone in the Rust
        // impl because mode=disabled means the zone is administratively
        // turned off, not that the CPU is being throttled. A "disabled"
        // zone present here must NOT influence the result.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = SysRoot::new(tmp.path());
        write_fixture(&root, "sys/class/thermal/thermal_zone0/mode", "disabled\n");
        write_fixture(&root, "sys/class/thermal/thermal_zone1/mode", "enabled\n");

        let status = read_throttle_status(&root).expect("read");
        assert_eq!(status, ThrottleStatus::default());
    }

    // -----------------------------------------------------------------
    // Fixture-tree tests — exercise the four readers against the
    // committed sys-typical disk fixtures so a fixture edit that breaks
    // the on-disk layout fails locally rather than only surfacing in
    // 2.4's integration tests.
    // -----------------------------------------------------------------

    fn typical_root() -> SysRoot {
        SysRoot::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sys-typical",
        ))
    }

    #[test]
    fn read_power_supplies_typical_fixture_tree() {
        let supplies = read_power_supplies(&typical_root()).expect("read fixture");
        assert_eq!(supplies.len(), 2);
        assert_eq!(supplies[0].name, "AC");
        assert_eq!(supplies[0].kind.as_deref(), Some("Mains"));
        assert!(
            supplies[0].capacity_pct.is_none(),
            "AC adapter has no capacity file"
        );
        assert_eq!(supplies[1].name, "BAT0");
        assert_eq!(supplies[1].kind.as_deref(), Some("Battery"));
        assert_eq!(supplies[1].capacity_pct, Some(87));
        assert_eq!(supplies[1].level.as_deref(), Some("Normal"));
        assert_eq!(supplies[1].power_uw, Some(12_500_000));
    }

    #[test]
    fn read_cpu_freq_info_typical_fixture_tree() {
        let info = read_cpu_freq_info(&typical_root()).expect("read fixture");
        assert_eq!(info.driver.as_deref(), Some("amd-pstate-epp"));
        assert_eq!(info.governor.as_deref(), Some("powersave"));
        assert_eq!(info.cur_freq_khz, Some(3_400_000));
        assert_eq!(info.min_freq_khz, Some(400_000));
        assert_eq!(info.max_freq_khz, Some(4_800_000));
        assert_eq!(info.epp.as_deref(), Some("balance_performance"));
        assert!(info.epp_available.is_some());
        assert_eq!(info.cpu_count, 16, "fixture has cpu0..cpu15");
    }

    #[test]
    fn read_thermal_info_typical_fixture_tree() {
        let mut readings = read_thermal_info(&typical_root()).expect("read fixture");
        // Sort by label for stable assertion ordering.
        readings.sort_by(|a, b| a.label.cmp(&b.label));
        assert_eq!(
            readings.len(),
            2,
            "fixture has temp1 (Tctl) + temp2 (CPU fallback)"
        );
        assert_eq!(readings[0].label, "CPU"); // temp2 has no label file
        assert_eq!(readings[0].source, "k10temp");
        assert!((readings[0].temp_c - 50.1).abs() < 0.001);
        assert_eq!(readings[1].label, "Tctl"); // temp1
        assert!((readings[1].temp_c - 52.4).abs() < 0.001);
    }

    #[test]
    fn read_throttle_status_typical_fixture_tree() {
        let status = read_throttle_status(&typical_root()).expect("read fixture");
        // sys-typical has cpu0/thermal_throttle/package_throttle_count = 0
        // and cpu1..cpu15 have no throttle files at all.
        assert_eq!(status.throttle_count, 0);
    }
}
