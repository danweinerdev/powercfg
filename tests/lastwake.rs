//! End-to-end snapshot tests for `powercfg lastwake` (task 4.2).
//!
//! Pattern follows `tests/sleepstates.rs` and `tests/devicequery.rs`:
//! spawn the compiled binary with `POWERCFG_SYSROOT` and
//! `POWERCFG_JOURNAL_FIXTURE` pointed at fixture files, snapshot stdout
//! via `insta`. The journal fixture seam (env-var-gated reader) lets us
//! exercise both populated and empty journal branches without touching
//! the live `journalctl`.
//!
//! Note on TZ: `chrono::Local` on Linux reads `/etc/localtime`, not
//! `$TZ`. The default mode's `Sleep time` / `Wake time` lines render
//! `DateTime<FixedOffset>` values whose offset is preserved verbatim
//! from the journal line, so they're TZ-stable and snapshottable. The
//! `Duration:` line (when both timestamps are present) is also offset-
//! invariant — `wake - sleep` is computed as a UTC delta.

use assert_cmd::Command;

fn powercfg() -> Command {
    Command::cargo_bin("powercfg").expect("binary should be built by cargo test")
}

fn stdout(output: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(output.get_output().stdout.clone()).expect("stdout should be UTF-8")
}

#[test]
fn lastwake_default() {
    // sys-typical has pm_wakeup_irq=9, proc/interrupts row 9 →
    // "9-fasteoi acpi". The journal fixture's last suspend entry is
    // 2025-04-29T01:15:18-0700 and last suspend exit is
    // 2025-04-29T08:22:44-0700 → duration 7h 7m 26s.
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-typical")
        .env(
            "POWERCFG_JOURNAL_FIXTURE",
            "tests/fixtures/journal-typical.log",
        )
        .arg("lastwake")
        .assert()
        .success();
    let out = stdout(&assertion);
    assert!(
        out.contains("Sleep time: 2025-04-29T01:15:18-0700"),
        "default mode should show the most recent suspend entry: {out}",
    );
    assert!(
        out.contains("Wake time:  2025-04-29T08:22:44-0700"),
        "default mode should show the most recent suspend exit: {out}",
    );
    assert!(
        out.contains("Duration:   7h 7m 26s"),
        "default mode should compute duration from sleep/wake delta: {out}",
    );
    assert!(
        out.contains("Wake IRQ: 9"),
        "default mode should show wake IRQ from sysfs: {out}",
    );
    assert!(
        out.contains("Device: 9-fasteoi acpi"),
        "default mode should resolve IRQ device via /proc/interrupts: {out}",
    );
    assert!(
        !out.contains("[KERNEL WAKE MESSAGES]"),
        "default mode should omit kernel-wake section: {out}",
    );
    assert!(
        !out.contains("[ENABLED ACPI WAKE DEVICES]"),
        "default mode should omit ACPI section: {out}",
    );
    assert!(
        !out.contains("[RECENT SLEEP/WAKE HISTORY]"),
        "default mode should omit history section: {out}",
    );
    insta::assert_snapshot!("lastwake_default", out);
}

#[test]
fn lastwake_verbose() {
    // -v adds the [ENABLED ACPI WAKE DEVICES] section (always renders,
    // empty list → "  None."). The [KERNEL WAKE MESSAGES] section
    // requires dmesg output to be non-empty — running dmesg from a test
    // environment under a fixture sysroot is best-effort, so we don't
    // assert on the kernel-wake content. ACPI section is deterministic:
    // the fixture has GPP0 (enabled, sysfs=pci:0000:00:01.1) and PWRB
    // (enabled, no sysfs); GPP8 is disabled.
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-typical")
        .env(
            "POWERCFG_JOURNAL_FIXTURE",
            "tests/fixtures/journal-typical.log",
        )
        .args(["lastwake", "-v"])
        .assert()
        .success();
    let out = stdout(&assertion);
    assert!(
        out.contains("[ENABLED ACPI WAKE DEVICES]"),
        "verbose mode should include ACPI section: {out}",
    );
    assert!(
        out.contains("  GPP0: S4 (pci:0000:00:01.1)"),
        "GPP0 (enabled) should render with its sysfs node: {out}",
    );
    assert!(
        out.contains("  PWRB: S4\n"),
        "PWRB (enabled, no sysfs) should render bare: {out}",
    );
    assert!(
        !out.contains("GPP8"),
        "GPP8 (disabled) must not appear in the enabled-only list: {out}",
    );
    // Note on snapshotting: [KERNEL WAKE MESSAGES] is host-dependent
    // (dmesg fails under restricted-kernel hosts and produces variable
    // content otherwise). To keep this snapshot stable, we strip the
    // section before snapshotting — assertions above pin the rest of
    // the verbose-only structure.
    let stripped = strip_kernel_wake_section(&out);
    insta::assert_snapshot!("lastwake_verbose", stripped);
}

/// Remove the optional `[KERNEL WAKE MESSAGES]` section (and its
/// trailing blank line) from `out`. The verbose snapshot otherwise
/// captures host-dependent dmesg content; everything else in verbose
/// mode is deterministic against the fixtures.
fn strip_kernel_wake_section(out: &str) -> String {
    let mut result = String::new();
    let mut lines = out.lines().peekable();
    while let Some(line) = lines.next() {
        if line == "[KERNEL WAKE MESSAGES]" {
            // Skip the underline + content lines until we hit a blank.
            for next in lines.by_ref() {
                if next.is_empty() {
                    break;
                }
            }
            // Drop the prior blank we just emitted (the "before" blank
            // from `writeln!(out)?` at the section boundary).
            if result.ends_with("\n\n") {
                result.pop();
            }
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}

#[test]
fn lastwake_history() {
    // -n 5 renders the [RECENT SLEEP/WAKE HISTORY] section with the
    // last 5 events from the 30-day window. The fixture has 6 events;
    // the slice drops the first one and renders Sleep/Wake/Sleep/Wake
    // /Sleep → wait, actually 6 - 5 = 1 dropped, so we see events 2..6.
    // events[1] is Wake (2025-04-01 07:45), events[2..5] alternate.
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-typical")
        .env(
            "POWERCFG_JOURNAL_FIXTURE",
            "tests/fixtures/journal-typical.log",
        )
        .args(["lastwake", "-n", "5"])
        .assert()
        .success();
    let out = stdout(&assertion);
    assert!(
        out.contains("[RECENT SLEEP/WAKE HISTORY]"),
        "history mode should include history section: {out}",
    );
    // Last 5 events: events[1..6] from the 6-event fixture.
    assert!(
        out.contains("2025-04-01T07:45:12-0700 - WAKE"),
        "earliest of the trailing-5 should be the 2025-04-01 wake: {out}",
    );
    assert!(
        out.contains("2025-04-29T08:22:44-0700 - WAKE"),
        "last event should be the 2025-04-29 wake: {out}",
    );
    // The very first event (2025-04-01 03:30 sleep) is OUTSIDE the
    // 5-event tail and must NOT appear.
    assert!(
        !out.contains("2025-04-01T03:30:21-0700"),
        "oldest event should be trimmed at -n 5: {out}",
    );
    insta::assert_snapshot!("lastwake_history", out);
}

#[test]
fn lastwake_empty_journal() {
    // Empty journal exercises the "Unknown" fallback for both sleep
    // and wake. sys-headless has no pm_wakeup_irq either, so the wake
    // IRQ branch also collapses to "Not available". The duration line
    // is suppressed (no timestamps to subtract).
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-headless")
        .env(
            "POWERCFG_JOURNAL_FIXTURE",
            "tests/fixtures/journal-empty.log",
        )
        .arg("lastwake")
        .assert()
        .success();
    let out = stdout(&assertion);
    assert!(
        out.contains("Sleep time: Unknown"),
        "empty journal → Sleep time: Unknown: {out}",
    );
    assert!(
        out.contains("Wake time:  Unknown (system may not have slept this boot)"),
        "empty journal → wake-fallback message: {out}",
    );
    assert!(
        out.contains("Wake IRQ: Not available"),
        "no pm_wakeup_irq file → Wake IRQ: Not available: {out}",
    );
    assert!(
        !out.contains("Duration:"),
        "no timestamps means no duration line: {out}",
    );
    insta::assert_snapshot!("lastwake_empty_journal", out);
}
