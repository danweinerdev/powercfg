//! End-to-end JSON output snapshot tests for task 5.2.
//!
//! Each test spawns the compiled binary with `--json` against a fixture
//! sysroot (and, for `lastwake`, a fixture journal) and snapshots the
//! parsed JSON via `insta::assert_json_snapshot!`. Round-trip parsing
//! through `serde_json::Value` confirms the output is valid JSON before
//! any structural assertions or snapshot diffs run.
//!
//! Three commands (`requests`, `lastwake -v`, `waketimers`) read from
//! live D-Bus / dmesg / pactl during their run; that data depends on
//! the dev/CI machine and would render snapshots flaky. The affected
//! arrays (`inhibitors`, `audio_streams`, `vms`, `kernel_messages`,
//! `wake_timers`, `all_timers`) are redacted to fixed placeholders so
//! the snapshot pins shape (key presence + array-vs-null distinction)
//! without pinning live content. The matching text-mode tests use the
//! same approach (see `tests/lastwake.rs`'s `strip_kernel_wake_section`
//! helper and the header-only assertions in `tests/cli_help.rs`).

use assert_cmd::Command;
use serde_json::Value;

fn powercfg() -> Command {
    Command::cargo_bin("powercfg").expect("binary should be built by cargo test")
}

/// Run the compiled binary, assert it exited successfully, and parse
/// stdout as `serde_json::Value`. Failing to parse here proves the
/// output isn't valid JSON — a hard contract violation.
fn run_json(args: &[&str], envs: &[(&str, &str)]) -> Value {
    let mut cmd = powercfg();
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let output = cmd.args(args).output().expect("command should run");
    assert!(
        output.status.success(),
        "{args:?} exited with {status:?}: stderr={stderr}",
        status = output.status,
        stderr = String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be UTF-8");
    serde_json::from_str::<Value>(&stdout)
        .unwrap_or_else(|e| panic!("--json output should parse as JSON: {e}\nstdout was: {stdout}"))
}

const SYS_TYPICAL: &str = "tests/fixtures/sys-typical";
const SYS_HEADLESS: &str = "tests/fixtures/sys-headless";
const JOURNAL_TYPICAL: &str = "tests/fixtures/journal-typical.log";
const JOURNAL_EMPTY: &str = "tests/fixtures/journal-empty.log";

// --- requests ----------------------------------------------------------

/// `requests --json` against sys-typical. The dev/CI host's live D-Bus,
/// pactl, and procfs queries return mutable content (running services,
/// active audio sinks, running VMs); redact those arrays so the
/// snapshot pins only key presence and the verbose-gated `usb_wakeup`
/// omission. Default mode (no `-v`) MUST omit `usb_wakeup` entirely.
#[test]
fn requests_default() {
    let v = run_json(
        &["requests", "--json"],
        &[("POWERCFG_SYSROOT", SYS_TYPICAL)],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    assert!(v["inhibitors"].is_array(), "inhibitors must be an array");
    assert!(v["wake_locks"].is_array(), "wake_locks must be an array");
    assert!(
        v["audio_streams"].is_array(),
        "audio_streams must be an array"
    );
    assert!(v["vms"].is_array(), "vms must be an array");
    // Verbose-gated key MUST be absent in default mode.
    assert!(
        v.get("usb_wakeup").is_none(),
        "usb_wakeup must be omitted when --verbose was not set: {v}",
    );
    insta::assert_json_snapshot!("requests_default", v, {
        ".inhibitors" => "[redacted-live-dbus]",
        ".wake_locks" => "[redacted-live-procfs]",
        ".audio_streams" => "[redacted-live-pactl]",
        ".vms" => "[redacted-live-procfs]",
    });
}

/// `requests --json -v` adds the `usb_wakeup` key. With sys-typical,
/// the USB walker finds two devices (`1-2`, `2-1`) so the array is
/// non-empty and snapshottable verbatim.
#[test]
fn requests_verbose() {
    let v = run_json(
        &["requests", "--json", "-v"],
        &[("POWERCFG_SYSROOT", SYS_TYPICAL)],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    assert!(
        v["usb_wakeup"].is_array(),
        "usb_wakeup must be an array in verbose mode: {v}",
    );
    insta::assert_json_snapshot!("requests_verbose", v, {
        ".inhibitors" => "[redacted-live-dbus]",
        ".wake_locks" => "[redacted-live-procfs]",
        ".audio_streams" => "[redacted-live-pactl]",
        ".vms" => "[redacted-live-procfs]",
    });
}

// --- lastwake ----------------------------------------------------------

/// `lastwake --json` against the typical fixture journal. Default mode
/// MUST NOT include the verbose-gated `kernel_messages` /
/// `acpi_enabled_devices` keys, nor the history-gated `history` key.
#[test]
fn lastwake_default() {
    let v = run_json(
        &["lastwake", "--json"],
        &[
            ("POWERCFG_SYSROOT", SYS_TYPICAL),
            ("POWERCFG_JOURNAL_FIXTURE", JOURNAL_TYPICAL),
        ],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    assert!(v["last_sleep"].is_string(), "last_sleep must be set");
    assert!(v["last_wake"].is_string(), "last_wake must be set");
    assert!(
        v["wake_irq"].is_object(),
        "wake_irq must be a populated object"
    );
    // Verbose-gated keys absent.
    assert!(
        v.get("kernel_messages").is_none(),
        "kernel_messages must be omitted in default mode: {v}",
    );
    assert!(
        v.get("acpi_enabled_devices").is_none(),
        "acpi_enabled_devices must be omitted in default mode: {v}",
    );
    assert!(
        v.get("history").is_none(),
        "history must be omitted in default mode: {v}",
    );
    insta::assert_json_snapshot!("lastwake_default", v);
}

/// `lastwake --json -v` adds `kernel_messages` and `acpi_enabled_devices`.
/// `kernel_messages` is the live `dmesg` output and is redacted to keep
/// the snapshot host-independent (matches the text-mode test's
/// `strip_kernel_wake_section` behavior). `acpi_enabled_devices` is
/// fixture-backed and snapshotted verbatim.
#[test]
fn lastwake_verbose() {
    let v = run_json(
        &["lastwake", "--json", "-v"],
        &[
            ("POWERCFG_SYSROOT", SYS_TYPICAL),
            ("POWERCFG_JOURNAL_FIXTURE", JOURNAL_TYPICAL),
        ],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    assert!(
        v["kernel_messages"].is_array(),
        "kernel_messages key must exist as an array in verbose mode: {v}",
    );
    assert!(
        v["acpi_enabled_devices"].is_array(),
        "acpi_enabled_devices key must exist as an array in verbose mode: {v}",
    );
    // History still absent without -n.
    assert!(
        v.get("history").is_none(),
        "history must remain absent without -n: {v}",
    );
    insta::assert_json_snapshot!("lastwake_verbose", v, {
        ".kernel_messages" => "[redacted-live-dmesg]",
    });
}

/// `lastwake --json -n 5` adds `history` (last 5 events from the 30-day
/// journal window). The fixture journal has 6 events, so `history`
/// holds the trailing 5 — fully fixture-backed.
#[test]
fn lastwake_history() {
    let v = run_json(
        &["lastwake", "--json", "-n", "5"],
        &[
            ("POWERCFG_SYSROOT", SYS_TYPICAL),
            ("POWERCFG_JOURNAL_FIXTURE", JOURNAL_TYPICAL),
        ],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    assert!(v["history"].is_array(), "history must be an array");
    assert_eq!(
        v["history"].as_array().unwrap().len(),
        5,
        "history must contain exactly 5 events from the trailing window: {v}",
    );
    // Verbose-gated keys still absent.
    assert!(v.get("kernel_messages").is_none());
    assert!(v.get("acpi_enabled_devices").is_none());
    insta::assert_json_snapshot!("lastwake_history", v);
}

/// Empty journal + headless sysroot: scalar fields render as `null`,
/// not omitted (the locked "data unavailable" signal).
#[test]
fn lastwake_empty_journal() {
    let v = run_json(
        &["lastwake", "--json"],
        &[
            ("POWERCFG_SYSROOT", SYS_HEADLESS),
            ("POWERCFG_JOURNAL_FIXTURE", JOURNAL_EMPTY),
        ],
    );
    assert!(v["last_sleep"].is_null(), "last_sleep must be null: {v}");
    assert!(v["last_wake"].is_null(), "last_wake must be null: {v}");
    assert!(v["wake_irq"].is_null(), "wake_irq must be null: {v}");
    insta::assert_json_snapshot!("lastwake_empty_journal", v);
}

// --- devicequery -------------------------------------------------------

/// `devicequery --json` is fully fixture-backed (procfs ACPI walk +
/// sysfs USB walk under `args.root`). The text printer's PCI-description
/// enrichment is intentionally skipped in JSON mode; this snapshot
/// confirms the model-only shape.
#[test]
fn devicequery_default() {
    let v = run_json(
        &["devicequery", "--json"],
        &[("POWERCFG_SYSROOT", SYS_TYPICAL)],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    assert!(
        v["acpi_devices"].is_array(),
        "acpi_devices must be an array"
    );
    assert!(v["usb_devices"].is_array(), "usb_devices must be an array");
    insta::assert_json_snapshot!("devicequery_default", v);
}

/// `devicequery --json -v` — verbose flag does NOT change the JSON
/// shape (the verbose enrichment is text-printer-only). The snapshot
/// is identical in shape to the default; we still pin it so a future
/// shape change behind `-v` would be caught.
#[test]
fn devicequery_verbose() {
    let v = run_json(
        &["devicequery", "--json", "-v"],
        &[("POWERCFG_SYSROOT", SYS_TYPICAL)],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    insta::assert_json_snapshot!("devicequery_verbose", v);
}

/// `devicequery --json --enabled-only` — like `-v`, the filter is a
/// text-printer concern (`acpi_devices` carries every row including
/// disabled ones in JSON, with the per-row `enabled` flag exposed).
#[test]
fn devicequery_enabled_only() {
    let v = run_json(
        &["devicequery", "--json", "--enabled-only"],
        &[("POWERCFG_SYSROOT", SYS_TYPICAL)],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    // Disabled devices remain in the JSON output — the filter is
    // text-only. The fixture's GPP8 has enabled=false; confirm it's
    // still present so we lock the JSON-vs-text divergence.
    let has_disabled = v["acpi_devices"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["enabled"] == false);
    assert!(
        has_disabled,
        "JSON must include disabled ACPI devices even with --enabled-only: {v}",
    );
    insta::assert_json_snapshot!("devicequery_enabled_only", v);
}

// --- sleepstates -------------------------------------------------------

/// `sleepstates --json` against sys-typical — fully fixture-backed.
/// The verbose-gated `image_size_bytes` key MUST be absent in default
/// mode.
#[test]
fn sleepstates_default() {
    let v = run_json(
        &["sleepstates", "--json"],
        &[("POWERCFG_SYSROOT", SYS_TYPICAL)],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    assert!(
        v.get("image_size_bytes").is_none(),
        "image_size_bytes must be omitted without --verbose: {v}",
    );
    insta::assert_json_snapshot!("sleepstates_default", v);
}

/// `sleepstates --json -v` adds `image_size_bytes`.
#[test]
fn sleepstates_verbose() {
    let v = run_json(
        &["sleepstates", "--json", "-v"],
        &[("POWERCFG_SYSROOT", SYS_TYPICAL)],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    assert!(
        v["image_size_bytes"].is_u64(),
        "image_size_bytes must be a number in verbose mode: {v}",
    );
    insta::assert_json_snapshot!("sleepstates_verbose", v);
}

// --- waketimers --------------------------------------------------------

/// `waketimers --json` — D-Bus succeeds on the dev/CI host so
/// `wake_timers` may be populated; redact it. RTC alarm comes from the
/// fixture sysroot (absent in sys-typical → `null`). The verbose-gated
/// `all_timers` key MUST be absent in default mode.
#[test]
fn waketimers_default() {
    let v = run_json(
        &["waketimers", "--json"],
        &[("POWERCFG_SYSROOT", SYS_TYPICAL)],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    assert!(v["wake_timers"].is_array(), "wake_timers must be an array");
    assert!(
        v.get("all_timers").is_none(),
        "all_timers must be omitted without --verbose: {v}",
    );
    insta::assert_json_snapshot!("waketimers_default", v, {
        ".wake_timers" => "[redacted-live-dbus]",
    });
}

/// `waketimers --json -v` adds `all_timers`. Both arrays come from
/// live D-Bus and are redacted; the snapshot pins key presence and
/// the rtc_wakealarm sysroot read.
#[test]
fn waketimers_verbose() {
    let v = run_json(
        &["waketimers", "--json", "-v"],
        &[("POWERCFG_SYSROOT", SYS_TYPICAL)],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    assert!(
        v["all_timers"].is_array(),
        "all_timers must be an array in verbose mode: {v}",
    );
    insta::assert_json_snapshot!("waketimers_verbose", v, {
        ".wake_timers" => "[redacted-live-dbus]",
        ".all_timers" => "[redacted-live-dbus]",
    });
}

// --- energy ------------------------------------------------------------

/// `energy --json` against sys-typical — fully fixture-backed (all
/// four sources read under `args.root`). Verbose flag does NOT change
/// the JSON shape.
#[test]
fn energy_default() {
    let v = run_json(&["energy", "--json"], &[("POWERCFG_SYSROOT", SYS_TYPICAL)]);
    assert!(v.is_object(), "root must be an object: {v}");
    assert!(v["supplies"].is_array());
    assert!(v["temperatures"].is_array());
    assert!(v["cpu"].is_object());
    assert!(v["throttle"].is_object());
    insta::assert_json_snapshot!("energy_default", v);
}

/// `energy --json -v` — `-v` only affects text output (an additional
/// `Available:` EPP line); the JSON shape is identical to default.
#[test]
fn energy_verbose() {
    let v = run_json(
        &["energy", "--json", "-v"],
        &[("POWERCFG_SYSROOT", SYS_TYPICAL)],
    );
    assert!(v.is_object(), "root must be an object: {v}");
    insta::assert_json_snapshot!("energy_verbose", v);
}
