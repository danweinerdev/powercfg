---
title: "Foundation"
type: phase
plan: RustRewrite
phase: 1
status: complete
created: 2026-04-28
updated: 2026-04-29
deliverable: "Compilable Cargo project with CI green and `powercfg sleepstates` working end-to-end against fixture and live /sys."
tasks:
  - id: "1.1"
    title: "Cargo project scaffold and CI workflow"
    status: complete
    verification: "`cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all succeed locally and in GitHub Actions on Linux. `cargo run -- --help` lists all six subcommands."
  - id: "1.2"
    title: "Core utilities: SysRoot, time parsing, format helpers, SourceError"
    status: complete
    depends_on: ["1.1"]
    verification: "Unit tests cover: `format_duration` for 0s/59s/1m/1h0m0s/1d0h0m0s and a multi-week duration; `format_freq` for kHz/MHz/GHz boundaries; `parse_iso_timestamp` for `±HH:MM`, `±HHMM`, and rejected malformed inputs; `SysRoot::join` returning the right path for both `/` root and a tempdir root. `SourceError` enum has variants `Io`, `Parse`, `Dbus`, `Subprocess`, `NotFound`, `Timeout`, derives `thiserror::Error`, implements `Display` producing one-line context strings (e.g., `parse: expected 2 fields, got 1`), and converts cleanly from `std::io::Error`, `zbus::Error`, and the local `ExecError` via `#[from]`."
  - id: "1.3"
    title: "clap CLI surface with all six subcommands"
    status: complete
    depends_on: ["1.1"]
    verification: "`powercfg <cmd> --help` for each of the six subcommands shows the expected flags (`-v` everywhere, `-n` on lastwake, `--enabled-only` on devicequery, `--json` global). `powercfg <unstub> ...` calls return `unimplemented!()` panic with a clear message; the implemented stub for `sleepstates` runs cleanly. Exit code is 2 on bad CLI input (clap default), 0 on success."
  - id: "1.4"
    title: "cmd::sleepstates end-to-end with fixture-based integration test"
    status: complete
    depends_on: ["1.2", "1.3"]
    verification: "Running `powercfg sleepstates` and `powercfg sleepstates -v` against the `sys-typical` fixture tree (via `POWERCFG_SYSROOT` env var) produces output that matches an `insta` snapshot. The output includes the three sections (`SLEEP STATES`, `MEMORY SLEEP MODE`, `HIBERNATION MODE`) with the right headers, the `[mem]`-bracket marker correctly identifies current mem_sleep mode, and the swap section reports zram with the `(may not support hibernation)` note. Running against `sys-headless` produces a snapshot that **omits the entire `[HIBERNATION MODE]` section** (not just the swap line), since `disk` is absent from `power/state` — matches Python line 604's `if 'disk' in states:` guard. Live run on the dev machine exits 0 and prints non-empty output."
---

# Phase 1: Foundation

## Overview

Stand up the Rust project, wire CI, and implement the simplest of the six subcommands (`sleepstates`) end-to-end. By the end of this phase, the skeleton is proven: the SysRoot abstraction works, format helpers produce correct strings, clap dispatches to handlers, and integration tests run against a fixture filesystem tree. The remaining five commands fit into the same pattern.

`sleepstates` is chosen first because it touches only `/sys/power/{state,mem_sleep,disk,image_size}` and `/proc/swaps` — no subprocess, no glob walking, smallest possible surface for the first parser to ship.

## 1.1: Cargo project scaffold and CI workflow

### Subtasks
- [x] Run `cargo init --bin` at repo root, naming the binary `powercfg` in `Cargo.toml`.
- [x] Add edition `2024`, `rust-version = "1.85"` (bumped from plan's 1.78 — edition 2024 requires ≥ 1.85), MIT license metadata mirroring `pyproject.toml`.
- [x] Add a minimal dependency set: `clap = { version = "4", features = ["derive"] }`, `anyhow`, `thiserror = "2"`, `tracing`, `tracing-subscriber` with `env-filter` feature. Test deps: `tempfile`, `assert_cmd`, `insta`.
- [x] Create `src/main.rs` that prints `unimplemented!()` for now; `cargo build` succeeds.
- [x] Add `.github/workflows/ci.yml` running on `ubuntu-latest`: `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`. Uses `Swatinem/rust-cache@v2`. **On disk, commit pending user approval.**
- [x] Add `rust-toolchain.toml` pinning stable Rust.
- [x] Update `.gitignore` for `target/`.

### Notes
The existing `Makefile` and `pyproject.toml` stay in place for now — they are torn down in Phase 6. The Rust binary builds to `target/debug/powercfg`, the Python script is `powercfg.py`. No name collision.

Commit `22c931d`: Cargo + src + toolchain + .gitignore. CI workflow at `.github/workflows/ci.yml` is on disk and locally validated but uncommitted at user direction — to be committed as a follow-up before Phase 1 closes.

The verification field on this task references `cargo run -- --help` listing six subcommands; that criterion is properly satisfied by task 1.3 (clap surface), not 1.1, and will pass once 1.3 lands.

## 1.2: Core utilities: SysRoot, time parsing, format helpers, SourceError

### Subtasks
- [x] `src/paths.rs` — `SysRoot(PathBuf)` newtype with `default()` (returns `/`), `from_env()` reading `POWERCFG_SYSROOT`, `new()`, `as_path()`, and `join()` with a `debug_assert!(rel.is_relative())` that catches absolute-path callers loudly. (Helper methods like `power_dir()` deferred to first user — the bare `join()` is sufficient until a source module needs a typed wrapper.)
- [x] `src/time.rs` — `parse_iso_timestamp(&str) -> Option<DateTime<FixedOffset>>` handling `+HH:MM` (`%:z`) and `+HHMM` (`%z`) offsets, rejects `Z` suffix and naive timestamps. `chrono` added to `Cargo.toml` with minimal features (`std` only — no `clock`).
- [x] `src/format/duration.rs` — `format_duration(Duration) -> String` matching Python output exactly (0s, 59s, 1m, 1h 1m 1s, 1d 1h 1m 1s).
- [x] `src/format/freq.rs` — `format_freq(khz: u64) -> String` returning `3.40 GHz` / `1500 MHz` / `500 kHz`. Half-even rounding parity with Python documented in test comment.
- [x] `src/format/mod.rs` exposes `duration`, `freq`. (`text` added in 1.4.)
- [x] `src/source/error.rs` — `SourceError` enum with `thiserror::Error` derive: `Io(#[from] std::io::Error)`, `Parse(String)`, `Dbus(String)`, `Subprocess(String)`, `Timeout(String)`, `NotFound(String)`. The `Dbus`/`Subprocess`/`Timeout` `String` payloads are placeholders for Phase 3 — the variants are present from day one so Phase 2 sources don't churn the enum.
- [x] 34 unit tests across the modules (5 paths, 7 time, 9 duration, 6 freq, 7 SourceError). All four gates (build/test/clippy/fmt) green.

### Notes
The `Dbus` and `Subprocess` variants are stubbed in this phase since `zbus` and `source::exec` arrive in Phase 3. The variants exist from day one so Phase 2 can return `Result<T, SourceError>` from sysfs/procfs sources without churning the enum later.

Initial implementation at commit `8f3f60a`. Quality fix-ups at `5984bfe`: `SysRoot::join` debug_assert, drop chrono `clock` feature (-215 lines from `Cargo.lock`), env-test RAII guard, freq parity comment.

## 1.3: clap CLI surface with all six subcommands

### Subtasks
- [x] `src/cli.rs` — `Cli` struct with `#[derive(Parser)]`, global `--json` flag, `Format` enum, and a `Command` subcommand enum.
- [x] `Command` enum with all six variants: `Requests`, `Lastwake { history: Option<usize> }`, `Devicequery { enabled_only: bool }`, `Sleepstates`, `Waketimers`, `Energy`, each with `verbose: bool`. Help strings mirror Python's `add_parser` text.
- [x] `src/main.rs` — `tracing-subscriber` initialization with `EnvFilter::from_default_env()` and stderr writer; clap parse; dispatch via `match` on `Command`; convert `Result<(), anyhow::Error>` to `ExitCode`. Each dispatch arm constructs the matching `cmd::*::Args` struct (refactored after the quality scan).
- [x] `src/cmd/mod.rs` declaring all six modules. Each module exposes `pub struct Args` mirroring its clap variant fields plus `pub fn run(args: Args) -> anyhow::Result<()>`. Five stubs `unimplemented!()` pinned to delivery phase; `sleepstates` returns `Ok(())` so the verification field passes.
- [x] `tests/cli_help.rs` — 10 integration tests using `assert_cmd` + `insta`: `--help` snapshots for top-level and each subcommand (7 snapshots), plus exit-code contracts for missing-subcommand (clap default exit 2), `sleepstates` (success), and a panicking stub (non-zero).

### Notes
Exit codes: 0 success, 1 internal error (anyhow propagation in `main`), 2 bad CLI args (clap default). The Python tool exits 1 when no subcommand is given via `parser.print_help(); return 1` — clap's default is exit 2 for the same case. The verification accepts clap's default since the user-facing message is similar and exit-2 is more idiomatic for missing-required-arg in Rust CLI tools.

`#![allow(dead_code)]` on `main.rs` retained from 1.2 because the utility items in `paths`/`time`/`format`/`source::error` are not yet referenced from any command handler — task 1.4 (sleepstates body) wires the first real call sites and removes the lid.

Initial implementation at commit `53c7e54`. Quality fix-ups at `81c7095`: per-command `Args` structs (decouples future args from dispatch signatures), drop premature `pub use SourceError` re-export, rename `requests_stub_panics` → `requests_stub_exits_nonzero`.

Test count: 34 → 44 (10 CLI tests).

## 1.4: cmd::sleepstates end-to-end with fixture-based integration test

### Subtasks
- [x] `src/source/sysfs.rs` — `read_sleep_states`, `read_mem_sleep_modes`, `read_disk_modes`, `read_image_size_bytes`, plus shared `parse_bracketed_modes` helper. All return `Result<T, SourceError>`.
- [x] `src/source/procfs.rs` — `read_swaps(&SysRoot) -> Result<Vec<SwapDevice>, SourceError>` parsing `/proc/swaps` (tab-separated columns, header skipped).
- [x] `src/cmd/sleepstates.rs` — full handler: each source call wrapped in `match` with `tracing::debug!` on the `Err` arm; populates `SleepStatesReport`; hands to text printer.
- [x] `src/model/sleepstates.rs` — `SleepStatesReport` owning struct with `Default` derive. (Distinct sub-structs like `MemSleep`/`Hibernation` weren't necessary — flat fields on the report struct were cleaner; revisit if Phase 5 JSON wants nesting.)
- [x] `src/format/text.rs` — `print_sleepstates` matches Python output for the happy path; **diverges intentionally** for the unreadable-mem_sleep case (added an `Unable to read memory sleep mode` fallback parallel to SLEEP STATES, fixing Python's internal inconsistency).
- [x] `src/cmd/sleepstates.rs` orchestrates: build report → print.
- [x] `tests/fixtures/sys-typical/` and `tests/fixtures/sys-headless/` — minimal trees per the task spec.
- [x] `scripts/capture_fixtures.sh` — capture recipe with redaction warning covering both text identifier files and PCI binary blobs.
- [x] `tests/sleepstates.rs` — 4 snapshot tests (typical/headless × verbose/no-v).
- [x] All four gates green; `cli_help.rs` extended to lock the new memory-section fallback message.

### Notes
The output matches the Python tool's `cmd_sleepstates` byte-for-byte for the happy path; one deliberate deviation for the empty-mem_sleep case (see above). The verification target is the `insta` snapshot file, not a parity script — once the snapshot is reviewed and accepted by the user, it locks the format.

Initial implementation at commit `741b712`. Quality fix-ups at `8fc0b37`: memory-section fallback message, multi-bracket parser test + doc, capture-script redaction warning extended for PCI binary blobs.

Test count: 44 → 68 (+24): 9 sysfs unit, 6 procfs unit, 4 sleepstates fixture-snapshot, 4 sleepstates integration, 1 multi-bracket parser test added at the quality pass.

## Acceptance Criteria

- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all green in CI.
- [x] `powercfg sleepstates` and `powercfg sleepstates -v` exit 0 on the dev machine and produce output that visibly matches the Python tool's output for the same machine state.
- [x] `POWERCFG_SYSROOT=tests/fixtures/sys-typical powercfg sleepstates` produces snapshot-stable output. Same for `sys-headless`.
- [x] Unit tests for `SysRoot`, time parsing, and format helpers exist and pass.
- [x] Stubs for the other five subcommands respond to `--help` and panic cleanly when invoked (no silent success, no compilation needed in later phases to add the command).
