//! Owning data structures for the `devicequery` subcommand.
//!
//! Phase 5 adds `#[derive(Serialize)]`; left bare for now to mirror
//! `model::sleepstates`.

/// One ACPI wake-capable device row from `/proc/acpi/wakeup`.
///
/// `state` is the literal S-state token (`"S3"`, `"S4"`, …) — left as a
/// free-form string at this layer because the kernel occasionally emits
/// values outside the documented set and the printer renders them
/// verbatim. `enabled` flattens the `*enabled` / `enabled` /
/// `*disabled` / `disabled` distinction; the leading `*` only marks
/// "currently capable", which we treat the same as plain `enabled`.
/// `sysfs` is `None` when the device row had no fourth column (e.g.
/// `PWRB`, `LID0` on some systems).
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WakeupStats {
    pub wakeup_count: u64,
    pub wakeup_active_count: u64,
    pub wakeup_last_time_ms: u64,
}

/// One USB wake-capable device discovered by the USB walker.
/// `device` is the sysfs name (e.g. `"1-2"`) and `name` is the
/// `<manufacturer> <product>` composition the printer renders.
#[derive(Debug, Clone)]
pub struct UsbWakeDevice {
    pub device: String,
    pub name: String,
}

/// Top-level report consumed by `format::text::print_devicequery`.
#[derive(Debug, Default)]
pub struct DeviceQueryReport {
    pub acpi_devices: Vec<AcpiWakeDevice>,
    pub usb_devices: Vec<UsbWakeDevice>,
}
