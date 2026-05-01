//! D-Bus access via `zbus::blocking`.
//!
//! Replaces the Python tool's `systemd-inhibit` and `systemctl` shellouts
//! with typed proxies on the system bus. Inhibitor queries hit
//! `org.freedesktop.login1.Manager`; timer-unit queries hit
//! `org.freedesktop.systemd1.Manager`. A single [`Connection`] is shared
//! across all calls within a command invocation.
//!
//! See `Designs/RustRewrite/README.md` Decision 3 for the rationale on
//! choosing zbus blocking over async + an executor.

use zbus::blocking::Connection;
use zbus::proxy;

use crate::model::requests::Inhibitor;
use crate::model::waketimers::TimerEntry;
use crate::source::SourceError;

/// Open a system-bus connection. Wraps `zbus::Error` so callers see
/// `SourceError::Dbus(_)` rather than reaching into zbus directly.
///
/// Construct once per command invocation and pass `&Connection` to the
/// per-call helpers below.
pub fn system_bus() -> Result<Connection, SourceError> {
    Ok(Connection::system()?)
}

/// `org.freedesktop.login1.Manager` proxy.
///
/// We expose only the surface this project needs: `ListInhibitors`. Add
/// methods here as future tasks need them. The `proxy!` macro generates
/// `LogindManagerProxyBlocking` for the blocking API.
#[proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait LogindManager {
    /// Returns `(what, who, why, mode, uid, pid)` per inhibitor.
    /// The 6-tuple shape is dictated by the D-Bus signature `a(ssssuu)`
    /// and the field order matches `Inhibit()` plus the trailing
    /// `uid, pid` pair (see <https://systemd.io/INHIBITOR_LOCKS>).
    #[allow(clippy::type_complexity)]
    fn list_inhibitors(&self) -> zbus::Result<Vec<(String, String, String, String, u32, u32)>>;
}

/// `org.freedesktop.systemd1.Manager` proxy.
///
/// `ListUnits` returns a 10-tuple per unit. We only consume `name` and
/// the per-unit object path; the rest are read into `_` placeholders.
#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait SystemdManager {
    /// Returns `(name, description, load_state, active_state, sub_state,
    /// followed_unit, object_path, job_id, job_type, job_object_path)`.
    /// The 10-tuple shape is dictated by the D-Bus signature
    /// `a(ssssssouso)`; flattening it into a struct would lose the
    /// codegen wiring `proxy!` provides.
    #[allow(clippy::type_complexity)]
    fn list_units(
        &self,
    ) -> zbus::Result<
        Vec<(
            String,
            String,
            String,
            String,
            String,
            String,
            zbus::zvariant::OwnedObjectPath,
            u32,
            String,
            zbus::zvariant::OwnedObjectPath,
        )>,
    >;
}

/// `org.freedesktop.systemd1.Timer` interface — per-unit. We read two
/// properties: `WakeSystem` and `NextElapseUSecRealtime`. The interface
/// is on a unit's object path, not the manager's, so callers must build
/// the proxy with the per-unit path.
#[proxy(
    interface = "org.freedesktop.systemd1.Timer",
    default_service = "org.freedesktop.systemd1"
)]
trait SystemdTimer {
    #[zbus(property)]
    fn wake_system(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn next_elapse_u_sec_realtime(&self) -> zbus::Result<u64>;
}

/// Call `org.freedesktop.login1.Manager.ListInhibitors` and convert each
/// `(what, who, why, mode, uid, pid)` tuple to a typed [`Inhibitor`]
/// (with `comm` resolved from `/proc/<pid>/comm`).
///
/// An empty inhibitor list is `Ok(vec![])`, not an error.
pub fn list_inhibitors(conn: &Connection) -> Result<Vec<Inhibitor>, SourceError> {
    let proxy = LogindManagerProxyBlocking::new(conn)?;
    let raw = proxy.list_inhibitors()?;
    Ok(raw.into_iter().map(Inhibitor::from_dbus_tuple).collect())
}

/// Call `org.freedesktop.systemd1.Manager.ListUnits`, filter to
/// `*.timer` units, and read `WakeSystem` + `NextElapseUSecRealtime`
/// off each unit's `Timer` interface using the same connection.
///
/// Per-unit property reads that fail (e.g. a unit without a real Timer
/// interface) fall back to `(false, 0)` rather than failing the whole
/// walk. The `0` sentinel is rendered as `n/a` by `print_waketimers`.
pub fn list_systemd_timers(conn: &Connection) -> Result<Vec<TimerEntry>, SourceError> {
    let manager = SystemdManagerProxyBlocking::new(conn)?;
    let units = manager.list_units()?;

    let mut timers = Vec::new();
    for (name, _desc, _load, _active, _sub, _follow, obj_path, _, _, _) in units {
        if !name.ends_with(".timer") {
            continue;
        }
        // Per-unit Timer-interface proxy. Build via the typed builder
        // so we can hand it the unit's object path.
        let timer = SystemdTimerProxyBlocking::builder(conn)
            .path(obj_path)?
            .build()?;
        // Property reads can fail if the unit isn't actually a timer
        // (rare, but possible during reload races) or if the unit
        // exists but doesn't expose the Timer interface. Failures
        // surface via tracing::debug! so they're visible under
        // RUST_LOG=debug — important because `wake_system` defaulting
        // to `false` on read failure means a misconfigured timer would
        // silently appear as "does not prevent wake."
        let wake = timer.wake_system().unwrap_or_else(|e| {
            tracing::debug!(unit = name.as_str(), "wake_system read failed: {e}");
            false
        });
        let next_us = timer.next_elapse_u_sec_realtime().unwrap_or_else(|e| {
            tracing::debug!(
                unit = name.as_str(),
                "next_elapse_u_sec_realtime read failed: {e}"
            );
            0
        });
        timers.push(TimerEntry {
            unit: name,
            wake_system: wake,
            next_elapse_realtime_us: next_us,
        });
    }
    Ok(timers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live integration test: hits the actual system bus. `#[ignore]`
    /// because CI containers may not have a system bus available; run
    /// with `cargo test -- --ignored` on the dev machine.
    #[test]
    #[ignore = "requires a live system D-Bus; run with --ignored"]
    fn list_inhibitors_against_live_system_bus() {
        let conn = system_bus().expect("system bus should be available");
        let result = list_inhibitors(&conn);
        assert!(
            result.is_ok(),
            "list_inhibitors should succeed against live bus: {:?}",
            result.err(),
        );
    }

    /// Live integration test for the timer walk. `#[ignore]` for the
    /// same reason as above.
    #[test]
    #[ignore = "requires a live system D-Bus; run with --ignored"]
    fn list_systemd_timers_against_live_system_bus() {
        let conn = system_bus().expect("system bus should be available");
        let result = list_systemd_timers(&conn);
        assert!(
            result.is_ok(),
            "list_systemd_timers should succeed against live bus: {:?}",
            result.err(),
        );
    }
}
