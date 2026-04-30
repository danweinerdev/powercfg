//! End-to-end snapshot tests for `powercfg devicequery` (task 2.2).
//!
//! Pattern follows `tests/sleepstates.rs`: spawn the compiled binary
//! with `POWERCFG_SYSROOT` pointed at a fixture tree, snapshot stdout
//! via `insta`. Fixtures: `tests/fixtures/sys-typical` carries 3 ACPI
//! rows plus 3 USB devices (one disabled, two enabled);
//! `tests/fixtures/sys-headless` carries 1 ACPI row and no USB tree at
//! all, exercising the `Ok(vec![])` parent-missing branch.

use assert_cmd::Command;

fn powercfg() -> Command {
    Command::cargo_bin("powercfg").expect("binary should be built by cargo test")
}

fn stdout(output: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(output.get_output().stdout.clone()).expect("stdout should be UTF-8")
}

#[test]
fn devicequery_typical() {
    // sys-typical has 3 ACPI rows (GPP0/GPP8/PWRB) + 2 enabled USB
    // devices (1-2, 2-1) and 1 disabled (1-3, excluded by the walker).
    // Summary: 4 enabled (GPP0, PWRB, 1-2, 2-1) of 5 total (3 ACPI + 2
    // USB after filtering disabled USB).
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-typical")
        .arg("devicequery")
        .assert()
        .success();
    insta::assert_snapshot!("devicequery_typical", stdout(&assertion));
}

#[test]
fn devicequery_typical_verbose() {
    // -v adds a `Wake count: 5` line under GPP0 (its PCI device's
    // wakeup_count is 5 in the fixture). GPP8 is disabled — the verbose
    // branch still runs the stats read for it but skips the line if
    // count is 0; in the fixture GPP8 has no PCI device dir so the
    // stats read errors out and is silently dropped. PWRB has no sysfs
    // so the stats read isn't attempted at all.
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-typical")
        .args(["devicequery", "-v"])
        .assert()
        .success();
    let out = stdout(&assertion);
    assert!(
        out.contains("Wake count: 5"),
        "verbose output should include GPP0 wake count: {out}",
    );
    insta::assert_snapshot!("devicequery_typical_verbose", out);
}

#[test]
fn devicequery_typical_enabled_only() {
    // --enabled-only filters the ACPI table (GPP8 row drops out) but
    // the summary still reads "4 of 5" — computed from the unfiltered
    // counts (this is the subtle Python-parity rule from the task spec).
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-typical")
        .args(["devicequery", "--enabled-only"])
        .assert()
        .success();
    let out = stdout(&assertion);
    assert!(
        !out.contains("GPP8"),
        "--enabled-only should drop GPP8 from the table: {out}",
    );
    assert!(
        out.contains("Wake-enabled devices: 4 of 5"),
        "summary should still use unfiltered totals: {out}",
    );
    insta::assert_snapshot!("devicequery_typical_enabled_only", out);
}

#[test]
fn devicequery_headless() {
    // sys-headless has 1 ACPI row (PWRB, enabled, no sysfs) and no
    // sys/bus/usb/devices tree at all. The reader returns Ok(vec![])
    // for the missing parent, so the [USB WAKE DEVICES] section is
    // suppressed entirely. Summary: 1 of 1.
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-headless")
        .arg("devicequery")
        .assert()
        .success();
    let out = stdout(&assertion);
    assert!(
        !out.contains("[USB WAKE DEVICES]"),
        "headless output should not include the USB section: {out}",
    );
    assert!(
        out.contains("Wake-enabled devices: 1 of 1"),
        "headless summary should be 1 of 1: {out}",
    );
    insta::assert_snapshot!("devicequery_headless", out);
}
