//! End-to-end snapshot tests for `powercfg energy` (task 2.4).
//!
//! Pattern follows `tests/devicequery.rs`: spawn the compiled binary
//! with `POWERCFG_SYSROOT` pointed at a fixture tree, snapshot stdout
//! via `insta`. Two fixture profiles cover the two branches of the
//! desktop/laptop split:
//!
//! - `sys-typical` carries 2 power supplies (AC + BAT0), full cpufreq
//!   data with EPP, 2 hwmon temp readings on a `k10temp` chip, and a
//!   throttle counter of 0 (so the `[THERMAL THROTTLING]` section
//!   stays suppressed).
//! - `sys-headless` has no `power_supply` directory (exercises the
//!   `No power supplies detected (desktop system)` fallback at Python
//!   line 967), no hwmon (no `[TEMPERATURES]` section), and a
//!   single-CPU `intel_pstate` cpufreq tree with no EPP files (so the
//!   `Energy preference` and `Available:` lines are both absent, even
//!   in `-v` mode).

use assert_cmd::Command;

fn powercfg() -> Command {
    Command::cargo_bin("powercfg").expect("binary should be built by cargo test")
}

fn stdout(output: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(output.get_output().stdout.clone()).expect("stdout should be UTF-8")
}

#[test]
fn energy_typical() {
    // Locks all four sections: 2 supplies (AC + BAT0 with capacity +
    // power_now), full CPU info (driver/governor/cur/min/max/epp/
    // cpu_count=16), 2 temps (Tctl + CPU fallback), and no throttling
    // section (count is 0).
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-typical")
        .arg("energy")
        .assert()
        .success();
    let out = stdout(&assertion);
    assert!(
        !out.contains("[THERMAL THROTTLING]"),
        "throttle_count is 0 in the fixture; section should be suppressed: {out}",
    );
    insta::assert_snapshot!("energy_typical", out);
}

#[test]
fn energy_typical_verbose() {
    // -v adds the `Available: …` EPP line under `Energy preference:`.
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-typical")
        .args(["energy", "-v"])
        .assert()
        .success();
    let out = stdout(&assertion);
    assert!(
        out.contains("    Available: default performance balance_performance balance_power power"),
        "verbose output should include the Available EPP line: {out}",
    );
    insta::assert_snapshot!("energy_typical_verbose", out);
}

#[test]
fn energy_headless() {
    // Headless: no power_supply tree, no hwmon, single CPU with
    // intel_pstate but no EPP. The `No power supplies detected` line
    // fires; both `Energy preference` and `[TEMPERATURES]` are absent.
    //
    // -v mode produces byte-identical output here (the kernel didn't
    // expose epp_available, so the `Available:` line stays absent),
    // so we run both modes against the same snapshot rather than
    // duplicating the snapshot file.
    for verbose_args in [&[][..], &["-v"][..]] {
        let assertion = powercfg()
            .env("POWERCFG_SYSROOT", "tests/fixtures/sys-headless")
            .arg("energy")
            .args(verbose_args)
            .assert()
            .success();
        let out = stdout(&assertion);
        assert!(
            out.contains("No power supplies detected (desktop system)"),
            "headless output should include the desktop fallback line: {out}",
        );
        assert!(
            !out.contains("[TEMPERATURES]"),
            "headless output should not include the temperatures section: {out}",
        );
        assert!(
            !out.contains("Energy preference"),
            "headless intel_pstate fixture has no EPP; line should be absent: {out}",
        );
        assert!(
            !out.contains("Available:"),
            "headless has no epp_available; Available line stays absent in both modes: {out}",
        );
        assert!(
            !out.contains("[THERMAL THROTTLING]"),
            "headless has no throttle counter; section should be suppressed: {out}",
        );
        insta::assert_snapshot!("energy_headless", out);
    }
}
