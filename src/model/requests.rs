//! Owning data structures for the `requests` subcommand.
//!
//! `RequestsReport` is the top-level report consumed by
//! `format::text::print_requests`. The leaf types are produced by
//! `source::dbus::list_inhibitors` (3.2), `source::procfs::find_processes_by_comm`
//! and `source::userspace::list_audio_streams` (3.3), and
//! `source::sysfs::read_kernel_wake_locks` / `read_usb_wakeup_devices` (2.2).

use crate::model::devicequery::UsbWakeDevice;

/// One sleep/idle inhibitor as returned by logind's `ListInhibitors`.
///
/// The `comm` field is resolved from `/proc/<pid>/comm` at construction
/// time (see [`Inhibitor::from_dbus_tuple`]) so the printed output stays
/// column-compatible with the Python tool's `Process: <comm> (PID: <pid>)`
/// line. If the proc lookup fails (process gone, permission denied), `comm`
/// falls back to the `who` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inhibitor {
    /// Service name from logind, e.g. "GNOME Settings Daemon".
    pub who: String,
    /// Free-form reason, e.g. "Playing audio".
    pub why: String,
    /// Colon-separated list, e.g. "sleep:idle" or
    /// "shutdown:sleep:idle:handle-power-key".
    pub what: String,
    /// "block" or "delay".
    pub mode: String,
    pub uid: u32,
    pub pid: u32,
    /// Resolved from `/proc/<pid>/comm` or fallback to `who`.
    pub comm: String,
}

impl Inhibitor {
    /// Build an `Inhibitor` from logind's `ListInhibitors` D-Bus reply
    /// tuple `(what, who, why, mode, uid, pid)`. The wire signature is
    /// `a(ssssuu)` and the field order follows the systemd docs at
    /// <https://systemd.io/INHIBITOR_LOCKS> — the same order
    /// `Inhibit()` accepts. Resolves `comm` from `/proc/<pid>/comm`;
    /// on failure (process gone, permission denied) falls back to a
    /// copy of `who`.
    pub fn from_dbus_tuple(t: (String, String, String, String, u32, u32)) -> Self {
        // Field order matches the live D-Bus reply, verified via
        // `busctl call ... ListInhibitors`: four strings (what, who,
        // why, mode) followed by uid then pid. `man systemd-inhibit`
        // and the systemd.io docs confirm this; the earlier draft had
        // who/why/what swapped and was caught only by live testing.
        let (what, who, why, mode, uid, pid) = t;
        let comm = resolve_comm(pid).unwrap_or_else(|| who.clone());
        Self {
            who,
            why,
            what,
            mode,
            uid,
            pid,
            comm,
        }
    }
}

/// Read `/proc/<pid>/comm` and trim the trailing newline. Returns `None`
/// on any I/O error (process gone, permission denied, non-Linux).
///
/// Unlike most procfs reads in this crate, this function ignores
/// `SysRoot` because the inhibitor list is a live system query — the
/// PIDs are real PIDs on the running system, not fixture data.
fn resolve_comm(pid: u32) -> Option<String> {
    let path = format!("/proc/{pid}/comm");
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_owned())
}

/// One running process matching a name lookup.
///
/// Produced by `source::procfs::find_processes_by_comm` and consumed by
/// the VM-detection path in `cmd::requests` (3.4). The `comm` field is
/// the kernel's truncated 15-character process name (the value in
/// `/proc/<pid>/comm`), not a full executable path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub comm: String,
}

/// One active PulseAudio/PipeWire sink-input.
///
/// Produced by `source::userspace::list_audio_streams` (which shells
/// out to `pactl list sink-inputs short`) and consumed by
/// `cmd::requests` (3.4). `id` is the sink-input ID (numeric in
/// practice but typed as `String` to match the raw column from
/// `pactl`'s output). `client` is the client name or `"Unknown"` if
/// the column is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioStream {
    pub id: String,
    pub client: String,
}

/// Top-level data for the `requests` subcommand. Built by
/// `cmd::requests::run` from D-Bus, procfs, and userspace sources.
#[derive(Debug, Default)]
pub struct RequestsReport {
    /// All inhibitors returned by logind, unfiltered. The display
    /// loop in `format::text::print_requests` filters to only
    /// sleep/idle entries when rendering, but counts the full list
    /// in the summary footer (matches Python).
    pub inhibitors: Vec<Inhibitor>,
    pub wake_locks: Vec<String>,
    pub audio_streams: Vec<AudioStream>,
    pub vms: Vec<ProcessInfo>,
    /// Only populated when verbose=true; rendered as the "[USB
    /// WAKEUP DEVICES]" section. The summary footer does NOT count
    /// these (matches Python).
    pub usb_wakeup: Vec<UsbWakeDevice>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Self-pid lookup: `/proc/<this-test's-pid>/comm` always exists,
    /// so `comm` should resolve from /proc rather than fall back to `who`.
    #[test]
    fn from_dbus_tuple_resolves_comm_from_proc_for_self_pid() {
        let pid = std::process::id();
        // Wire tuple order is (what, who, why, mode, uid, pid).
        let tuple = (
            "sleep:idle".to_owned(),
            "GNOME Settings Daemon".to_owned(),
            "Playing audio".to_owned(),
            "block".to_owned(),
            1000_u32,
            pid,
        );
        let inh = Inhibitor::from_dbus_tuple(tuple);

        assert_eq!(inh.who, "GNOME Settings Daemon");
        assert_eq!(inh.why, "Playing audio");
        assert_eq!(inh.what, "sleep:idle");
        assert_eq!(inh.mode, "block");
        assert_eq!(inh.uid, 1000);
        assert_eq!(inh.pid, pid);
        // /proc/self/comm is always readable for the running test
        // binary; it should not have fallen back to `who`.
        assert_ne!(
            inh.comm, "GNOME Settings Daemon",
            "comm should have resolved from /proc, not fallen back to `who`",
        );
        // And it should be non-empty (the test binary always has a comm).
        assert!(!inh.comm.is_empty(), "comm from /proc/self/comm is empty");
    }

    /// Empty `why` field — preserved verbatim, no special handling.
    #[test]
    fn from_dbus_tuple_preserves_empty_why() {
        // Wire tuple order is (what, who, why, mode, uid, pid).
        let tuple = (
            "sleep".to_owned(),
            "some.service".to_owned(),
            String::new(),
            "block".to_owned(),
            0_u32,
            std::process::id(),
        );
        let inh = Inhibitor::from_dbus_tuple(tuple);
        assert_eq!(inh.why, "");
    }

    /// Pid that definitely doesn't exist — `comm` falls back to `who`.
    #[test]
    fn from_dbus_tuple_falls_back_to_who_for_missing_pid() {
        // Wire tuple order is (what, who, why, mode, uid, pid).
        let tuple = (
            "idle".to_owned(),
            "stale.service".to_owned(),
            "Stale inhibitor".to_owned(),
            "delay".to_owned(),
            1000_u32,
            u32::MAX,
        );
        let inh = Inhibitor::from_dbus_tuple(tuple);
        assert_eq!(
            inh.comm, "stale.service",
            "expected fallback to `who` when /proc/<pid>/comm is unreadable",
        );
    }
}
