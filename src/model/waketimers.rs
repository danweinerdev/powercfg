//! Owning data structures for the `waketimers` subcommand.
//!
//! `WakeTimersReport` lands in task 3.5; for 3.2 we ship the leaf
//! `TimerEntry` type that `source::dbus::list_systemd_timers` produces.

/// One systemd timer unit, queried via
/// `org.freedesktop.systemd1.Manager.ListUnits` plus per-unit `Timer`
/// interface property reads.
///
/// `next_elapse_realtime_us` is microseconds since the Unix epoch
/// (systemd's `CLOCK_REALTIME`). `0` indicates "no next elapse" (timer
/// disabled or hasn't computed yet); `u64::MAX` is the systemd "no next"
/// sentinel for some monotonic timers leaking through. The 3.5
/// formatter renders both as `n/a`.
// TODO(phase-3.5): consumed by `cmd::waketimers`; drop allow once wired up.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerEntry {
    /// Unit name, always ends in `.timer`.
    pub unit: String,
    /// `WakeSystem` property — `true` if firing this timer wakes the
    /// machine from suspend (RTC-backed).
    pub wake_system: bool,
    /// `NextElapseUSecRealtime` property in microseconds since the Unix
    /// epoch. Sentinels: `0` = unscheduled, `u64::MAX` = no next.
    pub next_elapse_realtime_us: u64,
}
