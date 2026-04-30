//! Integration tests for the clap CLI surface (task 1.3).
//!
//! Snapshots `--help` output for the top-level command and each of the
//! six subcommands so accidental edits to the user-facing CLI shape
//! show up as snapshot diffs. Also exercises the dispatcher exit-code
//! contract: clean exit for the implemented stub (`sleepstates`),
//! non-zero exit for the panicking stubs and for missing-subcommand
//! input.

use assert_cmd::Command;

fn powercfg() -> Command {
    Command::cargo_bin("powercfg").expect("binary should be built by cargo test")
}

#[test]
fn top_level_help_lists_all_subcommands() {
    let output = powercfg().arg("--help").assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    insta::assert_snapshot!("help_top_level", stdout);
}

#[test]
fn requests_help() {
    let output = powercfg().args(["requests", "--help"]).assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    insta::assert_snapshot!("help_requests", stdout);
}

#[test]
fn lastwake_help() {
    let output = powercfg().args(["lastwake", "--help"]).assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    insta::assert_snapshot!("help_lastwake", stdout);
}

#[test]
fn devicequery_help() {
    let output = powercfg()
        .args(["devicequery", "--help"])
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    insta::assert_snapshot!("help_devicequery", stdout);
}

#[test]
fn sleepstates_help() {
    let output = powercfg()
        .args(["sleepstates", "--help"])
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    insta::assert_snapshot!("help_sleepstates", stdout);
}

#[test]
fn waketimers_help() {
    let output = powercfg().args(["waketimers", "--help"]).assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    insta::assert_snapshot!("help_waketimers", stdout);
}

#[test]
fn energy_help() {
    let output = powercfg().args(["energy", "--help"]).assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    insta::assert_snapshot!("help_energy", stdout);
}

#[test]
fn no_subcommand_fails() {
    // clap's default behavior when a required subcommand is missing is
    // to print an error to stderr and exit with code 2.
    powercfg().assert().failure().code(2);
}

#[test]
fn sleepstates_runs_cleanly() {
    // Task 1.4 fills the body; the assertion is now exit 0 plus a
    // recognizable header. Pointing at an empty SYSROOT (a tempdir
    // prefix that doesn't exist) makes every source read fail and
    // exercises the swallow-with-debug behavior — the handler still
    // prints headers and "Unable to read sleep states", still exits 0.
    let assertion = powercfg()
        .env("POWERCFG_SYSROOT", "/nonexistent-powercfg-test-root")
        .arg("sleepstates")
        .assert()
        .success();
    let stdout = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("AVAILABLE SLEEP STATES"),
        "sleepstates should print the section header: {stdout}",
    );
    assert!(
        stdout.contains("Unable to read sleep states"),
        "sleepstates with empty sysroot should fall back to the empty-data message: {stdout}",
    );
}

#[test]
fn requests_stub_exits_nonzero() {
    // Panicking stubs surface as non-zero exit. We don't snapshot the
    // panic message because Rust's panic format is not stable across
    // compiler versions. The contract being asserted is the observable
    // exit code, not the implementation mechanism — when the real handler
    // lands in phase 3, this test naturally inverts via the per-task work.
    powercfg().arg("requests").assert().failure();
}
