//! End-to-end snapshot tests for `powercfg sleepstates` (task 1.4).
//!
//! Each test spawns the compiled binary with `POWERCFG_SYSROOT` pointed
//! at a fixture tree and snapshots stdout via `insta`. The fixtures live
//! under `tests/fixtures/{sys-typical,sys-headless}` and are
//! hand-crafted; `scripts/capture_fixtures.sh` codifies how a contributor
//! would capture a fresh tree from real hardware.

use assert_cmd::Command;

fn powercfg() -> Command {
    Command::cargo_bin("powercfg").expect("binary should be built by cargo test")
}

fn stdout(output: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(output.get_output().stdout.clone()).expect("stdout should be UTF-8")
}

#[test]
fn sleepstates_typical() {
    // sys-typical has all three sections: SLEEP STATES (freeze/mem/disk),
    // MEMORY SLEEP MODE ([s2idle] deep), HIBERNATION MODE (platform with
    // partition + zram swap). No -v, so no descriptions and no
    // [HIBERNATION IMAGE] section.
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-typical")
        .arg("sleepstates")
        .assert()
        .success();
    insta::assert_snapshot!("sleepstates_typical", stdout(&assertion));
}

#[test]
fn sleepstates_typical_verbose() {
    // -v adds: per-state description lines (13-space indent), the
    // `Available:` disk-modes line under HIBERNATION MODE, and a
    // `[HIBERNATION IMAGE]` section showing 4_194_304_000 / (1024*1024)
    // = 4000 MB.
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-typical")
        .args(["sleepstates", "-v"])
        .assert()
        .success();
    insta::assert_snapshot!("sleepstates_typical_verbose", stdout(&assertion));
}

#[test]
fn sleepstates_headless() {
    // sys-headless has `power/state` = "freeze mem" — no "disk" token,
    // so the entire `[HIBERNATION MODE]` section must be omitted (Python
    // line 604 guard). Swap fallback is also absent because that line
    // lives inside the hibernation section. No image_size file, so no
    // `[HIBERNATION IMAGE]` either.
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-headless")
        .arg("sleepstates")
        .assert()
        .success();
    let out = stdout(&assertion);
    assert!(
        !out.contains("[HIBERNATION MODE]"),
        "headless output should not contain hibernation section: {out}",
    );
    assert!(
        !out.contains("[HIBERNATION IMAGE]"),
        "headless output should not contain image section: {out}",
    );
    insta::assert_snapshot!("sleepstates_headless", out);
}

#[test]
fn sleepstates_headless_verbose() {
    // Same shape as the non-verbose headless case, plus per-state
    // descriptions. `[HIBERNATION IMAGE]` is still absent because the
    // fixture has no `power/image_size` file.
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "tests/fixtures/sys-headless")
        .args(["sleepstates", "-v"])
        .assert()
        .success();
    let out = stdout(&assertion);
    assert!(
        !out.contains("[HIBERNATION MODE]"),
        "headless -v output should not contain hibernation section: {out}",
    );
    assert!(
        !out.contains("[HIBERNATION IMAGE]"),
        "headless -v output should not contain image section: {out}",
    );
    insta::assert_snapshot!("sleepstates_headless_verbose", out);
}
