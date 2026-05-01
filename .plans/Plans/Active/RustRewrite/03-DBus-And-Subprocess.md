---
title: "D-Bus and Subprocess Sources"
type: phase
plan: RustRewrite
phase: 3
status: complete
created: 2026-04-28
updated: 2026-04-30
deliverable: "`zbus::blocking` D-Bus access to logind and systemd-manager, bounded subprocess helper, /proc process walk, and `requests` + `waketimers` shipped end-to-end. `requests` audio output is enriched with application name + PID + binary (improving on Python's raw client-index display)."
tasks:
  - id: "3.1"
    title: "source::exec timeout helper"
    status: complete
    verification: "Unit tests cover: a fast-completing process returns its stdout/stderr/exit code; a process that exceeds the timeout is killed and returns `ExecError::Timeout` within `timeout + 200ms`; a non-existent binary returns `ExecError::NotFound` rather than hanging; the helper does not leak child processes (verify by checking `getpid()` of children is reaped). Before merging, `cargo deny check advisories` produces no `unmaintained` or `unsound` findings for the chosen timeout dependency. If `wait-timeout` triggers an advisory or fails to compile on edition 2024, the in-tree thread+kill fallback ships instead and a 2-line `// chose in-tree because: ...` comment is added to `exec.rs`. `ExecError` converts cleanly into `SourceError::Subprocess`."
  - id: "3.2"
    title: "source::dbus: zbus proxies for logind and systemd-manager"
    status: complete
    verification: "Add `zbus = { version = \"4\", default-features = false, features = [\"blocking-api\"] }` to `Cargo.toml`. `source::dbus::Connection::system()` connects to the system bus or returns `SourceError::Dbus` cleanly when no bus is available (e.g., in a minimal container). `LogindManagerProxy::list_inhibitors()` returns a typed `Vec<Inhibitor>` constructed from the D-Bus `a(ssssuu)` tuple; an empty result returns `Ok(vec![])` not an error. `SystemdManagerProxy::list_units()` returns timer units; per-unit `WakeSystem` property reads use the existing connection (no new fork). Unit tests cover: `Inhibitor::from_dbus_tuple` for typical input, empty `why` field, non-UTF8 byte sequences (rejected with `SourceError::Parse`); a mock `Connection` rejection returns `SourceError::Dbus`. Live integration test on the dev machine: `list_inhibitors` returns at least the GNOME/KDE session inhibitor present on most desktops."
  - id: "3.3"
    title: "Process walk and userspace audio source"
    status: complete
    depends_on: ["3.1"]
    verification: "`source::procfs::find_processes_by_comm(&SysRoot, &[\"qemu\", \"qemu-system-x86_64\", \"qemu-system-aarch64\", \"VBoxHeadless\"])` walks `/proc/`, filters to numeric directory names, reads `comm` for each, returns matching processes with PID + comm. Unit tests against a fixture `/proc` tree covering: numeric dirs, non-numeric dirs (skipped), missing `comm` files (skipped), trailing newlines in `comm`, comm exact-name match. The function never invokes `pgrep` — verified by reading `procfs.rs` and grepping the codebase for any `pgrep` literal. `source::userspace::list_audio_streams()` runs `pactl list sink-inputs short` via `source::exec::run_with_timeout` (5s); `pactl` missing returns empty Vec via `SourceError::Subprocess(NotFound)` swallowed at the call site; parsed output covers tab-separated rows."
  - id: "3.4"
    title: "cmd::requests"
    status: complete
    depends_on: ["3.2", "3.3"]
    verification: "`powercfg requests` and `powercfg requests -v` exit 0 on the dev machine and print the same six sections as the Python tool: SYSTEM INHIBITORS, KERNEL WAKE LOCKS, AUDIO STREAMS, VIRTUAL MACHINES, USB WAKEUP DEVICES (verbose only), and the trailing summary. The display loop iterates ALL inhibitors (the source layer does no filtering — matches Python lines 1175-1180) and prints only those whose `what` field contains `sleep` or `idle` (case-insensitive). The `Total sleep blockers found: N` line counts ALL inhibitors + wake_locks + audio + vms (matches Python line 1227, which sums the unfiltered inhibitor list). `No active sleep blockers detected.` appears when total is zero. Snapshot test against a fixed `RequestsReport` (no live D-Bus or subprocess in tests) is stable."
  - id: "3.5"
    title: "cmd::waketimers"
    status: complete
    depends_on: ["3.2"]
    verification: "Live `powercfg waketimers` and `powercfg waketimers -v` runs on the dev machine produce content matching the Python tool for the same machine state, completing within 10 seconds on a machine with ≤50 active timers. Wake-capable timers (`WakeSystem=true`) are listed in the primary section; verbose mode adds the all-timers table truncated at 15 rows with `... and N more timers`. RTC alarm reading converts epoch seconds to a `YYYY-MM-DD HH:MM:SS` string **in local time** (using `chrono::Local`, matches Python's `datetime.fromtimestamp` — verified by test runs with `TZ=America/Los_Angeles` and `TZ=Europe/Berlin` showing the expected offset). Unit tests for the `NextElapseUSecRealtime` µs-epoch → `chrono::DateTime<Local>` conversion cover `0` and `u64::MAX` boundary cases."
  - id: "3.6"
    title: "Enrich audio streams with application name + PID + binary"
    status: complete
    depends_on: ["3.4"]
    verification: "`source::userspace::list_audio_streams` switches from `pactl list sink-inputs short` to `pactl list sink-inputs` (verbose) and parses the property dictionary. `model::requests::AudioStream` gains `application_name: Option<String>`, `pid: Option<u32>`, `binary: Option<String>` populated from the `application.name`, `application.process.id`, and `application.process.binary` properties — all `Option` because some streams (system mixer, screen recording) don't carry process metadata. Unit tests for `parse_pactl_verbose` cover: typical Firefox + Spotify with full properties, a stream missing `application.process.id` (pid=None, name still present), a stream missing all `application.*` properties (every Option is None), multi-line property values (quoted strings with embedded newlines), and malformed entries (skipped). `print_requests` renders `Client: <name> (<binary>, PID: <pid>)` when name + pid + binary all present; degrades to `Client: <name> (PID: <pid>)` when binary missing, `Client: <name>` when only name, and `Client: <unknown>` when nothing. Existing 3.4 snapshots are updated to the richer format. A `#[ignore]`-gated live test confirms the parser handles real `pactl list sink-inputs` output on the dev machine."
---

# Phase 3: D-Bus and Subprocess Sources

## Overview

Adds the data-acquisition infrastructure for everything that isn't sysfs/procfs:
- **D-Bus** via `zbus::blocking` for logind inhibitors and systemd timer units. Replaces what the Python tool does via `systemd-inhibit` and `systemctl` shellouts.
- **Subprocess** via `source::exec` for `pactl` (no clean typed alternative). Phase 4 reuses this for `journalctl` and `dmesg`.
- **`/proc` walk** for VM detection. Replaces what Python does via `pgrep`.

After this phase, `requests` and `waketimers` are live. Phase 4 adds `lastwake`.

This phase is the largest reduction in subprocess surface vs. the Python tool: 4 of the Python tool's 7 external command shellouts (`systemd-inhibit`, `systemctl list-timers`, `systemctl show`, `pgrep`) become typed in-process calls. Only `pactl` remains as subprocess (Phase 4 keeps `journalctl` and `dmesg` for the same reason).

## 3.1: source::exec timeout helper

### Subtasks
- [x] `src/source/exec.rs` — `run_with_timeout` with typed `ExecError { NotFound, Timeout, Io }`. Concurrent threaded pipe drain (mirrors `Child::wait_with_output` to avoid the 64 KB pipe-buffer deadlock — important for Phase 4's `journalctl` which produces megabytes).
- [x] `wait-timeout` 0.2.1 added to `Cargo.toml`. Builds clean on edition 2024 / Rust 1.95; `cargo deny check advisories` runs clean — no in-tree fallback needed.
- [x] `source::error::SourceError::Subprocess` evolves from `String` placeholder to `#[from] ExecError`; the standalone `Timeout(String)` variant is dropped (`Timeout` is reachable via `Subprocess(ExecError::Timeout)`).
- [x] 7 unit tests: fast success, timeout-kill-and-reap, NotFound, stderr capture, subsequent-call-not-blocked, large-stdout-no-deadlock, non-executable-file-Io.

### Notes
The helper always overrides stdin (`Stdio::null()`), stdout, and stderr — callers can't pass through. Stdin null-ing is intentional for pactl/journalctl/dmesg (none read stdin); future callers needing stdin would need a new API.

Initial implementation at commit `23fda86`. Quality fix-ups at `6eee8a9`: concurrent threaded pipe drain, `tracing::debug!` on `wait()` failure after kill, doc on `Io` and `Timeout` pipe-discard semantics, `ExecError::Io` test via permission-denied.

Test count: 132 → 140 (+8: 7 in `exec.rs`, 1 conversion test in `error.rs`).

## 3.2: source::dbus — zbus proxies for logind and systemd-manager

### Subtasks
- [x] `zbus = "5"` with `default-features = false`, `features = ["async-io", "blocking-api"]` (the `async-io` backend is required — `blocking-api` alone doesn't compile, but it's the lightest executor and keeps tokio out of the tree).
- [x] `src/source/dbus.rs` — `system_bus()` opens the connection once per command invocation; `Connection` is passed by reference.
- [x] `LogindManagerProxyBlocking` (via `#[proxy]` macro) for `org.freedesktop.login1.Manager.ListInhibitors`.
- [x] `pub fn list_inhibitors(&Connection) -> Result<Vec<Inhibitor>, SourceError>` — converts each `(who,why,what,mode,uid,pid)` tuple via `Inhibitor::from_dbus_tuple`. `comm` resolved from `/proc/<pid>/comm`; falls back to `who` if the read fails.
- [x] `SystemdManagerProxyBlocking` for `ListUnits()`. `SystemdTimerProxyBlocking` for per-unit `WakeSystem` + `NextElapseUSecRealtime` property reads, on the existing connection.
- [x] `pub fn list_systemd_timers(&Connection) -> Result<Vec<TimerEntry>, SourceError>` — filter to `.timer` units, read both properties. Per-unit failures surface via `tracing::debug!` (added in quality fix-up).
- [x] `model::requests::Inhibitor` and `model::waketimers::TimerEntry`. `WakeTimer` from earlier draft dropped — `TimerEntry` plus a downstream filter (3.5) is sufficient.
- [x] 3 unit tests for `Inhibitor::from_dbus_tuple` (self-pid, empty why, nonexistent pid fallback). 2 `#[ignore]`-gated live D-Bus integration tests.
- [x] `SourceError::Dbus` evolves from `String` to `#[from] zbus::Error`.

### Notes
Resolving inhibitor `pid` to `comm` keeps the Rust output column-compatible with the Python tool's `Process: <comm> (PID: <pid>)` line, even though the D-Bus API doesn't return `comm` directly.

Initial implementation at commit `ca7c3f5`. Quality fix-up at `ec40beb`: replace silent `unwrap_or` on Timer property reads with `tracing::debug!`-logged fallbacks so misconfigured timers don't silently report as non-wake. Skipped the `#[ignore]`'d-test timeout suggestion (adding `ntest` for two manually-run tests is over-budget).

Test count: 140 → 144 (+4 unit tests; 2 ignored live tests).

## 3.3: Process walk and userspace audio source

### Subtasks
- [x] `procfs::find_processes_by_comm` — walks numeric PID dirs in `<root>/proc/`, exact-match against caller-supplied names. `debug_assert!` enforces the 15-char `comm` truncation limit.
- [x] `model::requests::{ProcessInfo, AudioStream}` — owning structs.
- [x] `userspace::list_audio_streams` — `pactl list sink-inputs short` via `run_with_timeout(5s)`. Tab-separated parser split into `parse_pactl_short` so it's testable without subprocess. Includes a pinned test for the silent-skip-on-space-separated degradation path.
- [x] `sysfs::read_kernel_wake_locks` — single read, no TOCTOU precheck. `NotFound` and `PermissionDenied` both fold to `Ok(vec![])`.
- [x] 22 unit tests across the three modules + 1 `#[ignore]`-gated live `pactl` test.
- [x] `! grep -rn 'pgrep' src/` returns empty.

### Notes
The previous draft had `source::userspace::list_running_vms` shelling out to `pgrep`. That function is gone. VM detection lives in `procfs::find_processes_by_comm`, sharing a code path with any future "is process X running" lookup.

Initial implementation at commit `87ba85a`. Quality fix-ups at `4f437c9`: drop TOCTOU `path.exists()` precheck in `read_kernel_wake_locks`, add `debug_assert!` for 15-char `comm` limit in `find_processes_by_comm`, pin the space-separator degradation test, add `#[ignore]`'d live `pactl` test mirroring the dbus.rs live tests.

Test count: 144 → 168 (+24). 3 ignored tests now (2 D-Bus + 1 pactl live).

## 3.4: cmd::requests

### Subtasks
- [x] `cmd::requests::run` wires four sources (dbus inhibitors, kernel wake locks, pactl audio streams, /proc VM walk) plus optional USB wakeup in verbose mode. All errors swallowed at the call site with `tracing::debug!`.
- [x] `format::text::print_requests` takes `&mut dyn Write` (writer-pattern divergence from the other printers) so it's unit-testable against a `Vec<u8>` without going through the binary. Snapshot tests cover typical / typical-verbose / empty / all-non-sleep-inhibitors.
- [x] **Discovered + fixed a 3.2 wire-format bug**: `Inhibitor::from_dbus_tuple` had the field destructure as `(who, why, what, ...)` but logind actually returns `(what, who, why, ...)`. Caught only via live D-Bus testing because the 3.2 unit tests fed arbitrary strings.
- [x] **Diverges from Python**: the inhibitor `User:` line shows the numeric `uid` (e.g. `User: 1000`), not the resolved username Python gets from `systemd-inhibit`. logind D-Bus only exposes `uid: u32`; resolving to a name would need `getpwuid_r` and isn't worth the cost for a status command.
- [x] `VM_COMM_NAMES` covers `qemu` (broad), the four QEMU arch-specific 15-char comm forms (x86_64/aarch64/arm/riscv64/ppc64), and `VBoxHeadless`.

### Notes
The Python tool's `cmd_requests` total counts inhibitors, wake_locks, audio, vms — but not USB devices. Matched. The "filter at display, count at source" pattern preserved.

Initial implementation at commit `460655a`. Quality fix-ups at `de6ff8f`: extend `VM_COMM_NAMES` for ARM/RISC-V/PPC QEMU, add the all-non-sleep-inhibitor snapshot test, document the (uid, pid) field order with a live-verification reference.

The audio output still shows raw `pactl` client indexes (e.g. `Client: 176`) instead of application names — the printer is faithfully rendering what `parse_pactl_short` exposes, but the underlying short-form `pactl` output doesn't carry process metadata. Task 3.6 enriches this.

Test count: 168 → 169 (+1 snapshot for the all-non-sleep case).

## 3.5: cmd::waketimers

### Subtasks
- [x] `sysfs::read_rtc_wakealarm` — single read, formats non-zero epochs via `chrono::Local`. `0` and empty file → `Ok(None)`; non-numeric → `Err(Parse)`; missing/perm-denied → `Err(Io)`.
- [x] `cmd::waketimers::run` — opens single D-Bus connection, partitions via free `wake_capable()` helper (extracted for testability at quality fix-up), reads RTC alarm, prints.
- [x] `format::text::print_waketimers` — writer-pattern (`&mut dyn Write`) matching `print_requests`. 15-row cap with `... and N more timers` suffix, `n/a` for `0`/`u64::MAX` sentinels.
- [x] `TimerEntry::next_elapse_display` — µs-epoch → `chrono::Local` formatted string. Sentinels return `"n/a"`.
- [x] Re-added chrono `clock` feature (dropped in 1.2 quality fix; this task is the genuine consumer).
- [x] 14 tests added: 6 sysfs (RTC alarm parsing + format), 3 model (next_elapse_display sentinels + format), 4 format (print_waketimers shape), 1 cli_help (inverted from stub-fails to success). +3 more at quality fix-up for `wake_capable` partition.
- [x] Live `cargo run -- waketimers` and `-v` exit 0 in ~110ms on the dev machine.

### Notes
**Diverges from Python on `%Z`**: chrono renders `%Z` as a numeric offset (`+00:00`, `-07:00`) when `iana-time-zone` can't resolve a zone name. Python always produces an abbreviation (`PDT`). Both are valid timestamps. If abbreviation parity becomes important, `chrono-tz` is the path — not a current dep.

**chrono::Local on Linux ignores `$TZ`** — chrono ≥0.4.20 reads `/etc/localtime` via `iana-time-zone`. The test suite acknowledges this: timestamp-bearing tests use substring/structural assertions instead of `insta` snapshots; the empty-report test uses exact equality (no datetime strings).

Initial implementation at commit `c5747ec`. Quality fix-ups at `6205a1b`: corrected `set_tz` test comment, doc-noted the `%Z` numeric fallback, extracted `wake_capable` for unit-testable filter partition.

Test count: 169 → 186 (+17). 3 ignored tests (live D-Bus + live pactl).

## 3.6: Enrich audio streams with application name + PID + binary

The current `pactl list sink-inputs short` parser only exposes the raw client *index* (`Client: 176`), which is a PulseAudio/PipeWire internal handle, not a process identifier. Live output from a real desktop ends up unreadable:

```
[AUDIO STREAMS]
  Stream ID: 449
    Client: 176
  Stream ID: 7781
    Client: 7775
```

This task switches to the verbose `pactl list sink-inputs` form and parses the property dictionary it embeds, replacing each client index with the application's name, PID, and binary where available. The Python tool has the same gap; this is a deliberate Rust UX improvement that lands before Phase 5 so the JSON schema picks up the richer fields from day one.

### Subtasks
- [x] `source::userspace::list_audio_streams` switched from `pactl list sink-inputs short` to verbose `pactl list sink-inputs`.
- [x] `parse_pactl_verbose` walks `Sink Input #N` blocks, scans for `Properties:`, extracts `application.name`, `application.process.id`, `application.process.binary`. PipeWire-compat (space-indent) and PulseAudio (tab-indent) both work via `trim_start()`.
- [x] `AudioStream` rewritten — dropped legacy `client: String`, gained `application_name: Option<String>`, `pid: Option<u32>`, `binary: Option<String>`.
- [x] `format::text::format_audio_client` helper covers all 8 populated/missing combinations of name/binary/pid. Graceful 4-tier degradation from `Firefox (firefox, PID: 12345)` to `<unknown>`.
- [x] 9 parser unit tests + 8 formatter arm tests added; the 4 existing `print_requests` snapshots regenerated.
- [x] `#[ignore]`'d live-daemon test still passes (it only asserts Ok-shape).
- [x] Empty-string property values guarded — `application.name = ""` doesn't render as a leading-space artifact.

### Notes
**Diverges from Python deliberately**: Python uses the same `pactl ... short` and inherits the same unreadable raw-index display. This is a Rust UX improvement.

Live output on the dev machine confirms the fix: `Client: Firefox (firefox, PID: 13823)` for all 5 streams instead of bare numeric indexes.

Initial implementation at commit `bc1e84d`. Quality fix-ups at `989a27b`: empty-value guard in `set_property`, PipeWire space-indent test, accurate `parse_property_line` docstring on escape behavior.

Phase 5 (`--json`) will serialize the three new `Option<>` fields with `skip_serializing_if = "Option::is_none"`. A fully-populated stream becomes `{ "id": "449", "application_name": "Firefox", "pid": 12345, "binary": "firefox" }`; a system-stream collapses to just `{ "id": "99" }`.

Test count: 186 → 196 (+10).

## Acceptance Criteria

- [ ] `source::exec::run_with_timeout` correctly bounds slow processes and reaps children.
- [ ] `source::dbus` connects to the system bus and reads inhibitors and timer units without spawning subprocesses.
- [ ] `grep -r 'pgrep' src/` returns nothing.
- [ ] `grep -r 'systemd-inhibit' src/` returns nothing (zbus replaces it).
- [ ] `grep -r '"systemctl"' src/` returns nothing in production code (zbus replaces it).
- [ ] `powercfg requests` and `powercfg requests -v` exit 0 on the dev machine; output matches Python tool content; snapshot test against fixed mock data is stable.
- [ ] `powercfg waketimers` and `powercfg waketimers -v` exit 0 on the dev machine; live RTC alarm read works; total run time ≤10s with ≤50 active timers.
- [ ] All sources return `Result<T, SourceError>`; the command layer applies `.unwrap_or_default()` / `.ok()` at call sites with `.inspect_err` for debug logging.
- [ ] `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all green.
- [ ] `cargo deny check advisories` does not fire `unmaintained` for `wait-timeout` (or the in-tree fallback is in use, with a comment explaining why).
- [ ] `lastwake` stub still panics cleanly; other implemented commands still pass their snapshots.
