//! Owning data structures for the `requests` subcommand.
//!
//! `RequestsReport` lands in task 3.4; for 3.2 we ship the leaf
//! `Inhibitor` type that `source::dbus::list_inhibitors` produces.

/// One sleep/idle inhibitor as returned by logind's `ListInhibitors`.
///
/// The `comm` field is resolved from `/proc/<pid>/comm` at construction
/// time (see [`Inhibitor::from_dbus_tuple`]) so the printed output stays
/// column-compatible with the Python tool's `Process: <comm> (PID: <pid>)`
/// line. If the proc lookup fails (process gone, permission denied), `comm`
/// falls back to the `who` field.
// TODO(phase-3.4): consumed by `cmd::requests`; drop allow once wired up.
#[allow(dead_code)]
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

#[allow(dead_code)] // TODO(phase-3.4): consumed by `cmd::requests`.
impl Inhibitor {
    /// Build an `Inhibitor` from logind's `ListInhibitors` D-Bus reply
    /// tuple `(who, why, what, mode, uid, pid)`. Resolves `comm` from
    /// `/proc/<pid>/comm`; on failure (process gone, permission denied)
    /// falls back to a copy of `who`.
    pub fn from_dbus_tuple(t: (String, String, String, String, u32, u32)) -> Self {
        let (who, why, what, mode, uid, pid) = t;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Self-pid lookup: `/proc/<this-test's-pid>/comm` always exists,
    /// so `comm` should resolve from /proc rather than fall back to `who`.
    #[test]
    fn from_dbus_tuple_resolves_comm_from_proc_for_self_pid() {
        let pid = std::process::id();
        let tuple = (
            "GNOME Settings Daemon".to_owned(),
            "Playing audio".to_owned(),
            "sleep:idle".to_owned(),
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
        let tuple = (
            "some.service".to_owned(),
            String::new(),
            "sleep".to_owned(),
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
        let tuple = (
            "stale.service".to_owned(),
            "Stale inhibitor".to_owned(),
            "idle".to_owned(),
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
