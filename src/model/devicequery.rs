//! Owning data structures for the `devicequery` subcommand.

use serde::Serialize;

/// One ACPI wake-capable device row from `/proc/acpi/wakeup`.
///
/// `state` is the literal S-state token (`"S3"`, `"S4"`, …) — left as a
/// free-form string at this layer because the kernel occasionally emits
/// values outside the documented set and the printer renders them
/// verbatim. `enabled` flattens the `*enabled` / `enabled` /
/// `*disabled` / `disabled` distinction; the leading `*` only marks
/// "currently capable", which we treat the same as plain `enabled`.
/// `sysfs` is `None` when the device row had no fourth column (e.g.
/// `PWRB`, `LID0` on some systems) — JSON emits `null` to preserve
/// the unavailability signal rather than omitting.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AcpiWakeDevice {
    pub device: String,
    pub state: String,
    pub enabled: bool,
    pub sysfs: Option<String>,
}

/// Counters under `/sys/bus/pci/devices/<addr>/power/wakeup_*`.
///
/// `Default` returns all-zero, useful for the "device exists but
/// counters are unreadable" branch when callers degrade gracefully.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WakeupStats {
    pub wakeup_count: u64,
    pub wakeup_active_count: u64,
    pub wakeup_last_time_ms: u64,
}

/// One USB wake-capable device discovered by the USB walker.
/// `device` is the sysfs name (e.g. `"1-2"`) and `name` is the
/// `<manufacturer> <product>` composition the printer renders.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct UsbWakeDevice {
    pub device: String,
    pub name: String,
}

/// Top-level report consumed by `format::text::print_devicequery`.
///
/// JSON contract: both collections always serialize as arrays
/// (possibly empty). The schema's per-device `description` and
/// `stats` fields are computed by the text printer reaching back
/// into sysfs and are not stored on the model — JSON output omits
/// them. The schema's `totals` summary is also computed by the
/// text printer and not stored on the model.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DeviceQueryReport {
    pub acpi_devices: Vec<AcpiWakeDevice>,
    pub usb_devices: Vec<UsbWakeDevice>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip JSON: empty report has both keys as `[]`, never
    /// omitted.
    #[test]
    fn devicequery_report_empty_renders_arrays() {
        let report = DeviceQueryReport::default();
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["acpi_devices"].is_array());
        assert!(v["usb_devices"].is_array());
        assert_eq!(v["acpi_devices"].as_array().unwrap().len(), 0);
        assert_eq!(v["usb_devices"].as_array().unwrap().len(), 0);
    }

    /// Populated report: device rows render with snake_case keys
    /// matching the design schema; `sysfs: None` renders as `null`,
    /// not omitted.
    #[test]
    fn devicequery_report_populated_renders_expected_keys() {
        let report = DeviceQueryReport {
            acpi_devices: vec![
                AcpiWakeDevice {
                    device: "GPP0".into(),
                    state: "S4".into(),
                    enabled: true,
                    sysfs: Some("pci:0000:00:01.1".into()),
                },
                AcpiWakeDevice {
                    device: "PWRB".into(),
                    state: "S4".into(),
                    enabled: true,
                    sysfs: None,
                },
            ],
            usb_devices: vec![UsbWakeDevice {
                device: "1-2".into(),
                name: "Logitech Receiver".into(),
            }],
        };
        let json = serde_json::to_string(&report).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(v["acpi_devices"][0]["device"], "GPP0");
        assert_eq!(v["acpi_devices"][0]["state"], "S4");
        assert_eq!(v["acpi_devices"][0]["enabled"], true);
        assert_eq!(v["acpi_devices"][0]["sysfs"], "pci:0000:00:01.1");
        // `sysfs: None` → null (preserve unavailability signal).
        assert!(v["acpi_devices"][1]["sysfs"].is_null());

        assert_eq!(v["usb_devices"][0]["device"], "1-2");
        assert_eq!(v["usb_devices"][0]["name"], "Logitech Receiver");
    }
}
