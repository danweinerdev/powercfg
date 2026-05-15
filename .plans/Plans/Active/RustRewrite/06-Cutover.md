---
title: "Cutover"
type: phase
plan: RustRewrite
phase: 6
status: complete
created: 2026-04-28
updated: 2026-05-14
debriefs:
  - task: "6.1"
    commit: "cbb2f38"
    fix_up: "6ef5e56"
    notes: "Phase 6 dispatched before Phase 5 (per user). Skipped --json README section (deferred to post-Phase 5) and skipped task 6.2 (.github/ remains user-held). Quality scan caught one Major: README's Commands/Example/Equivalents sections still used `./powercfg.py` invocations (would have left users with No-such-file errors); replaced with bare `powercfg`. Also added /dist/ to .gitignore as forward protection for 6.2's cargo-dist output. 234 tests pass; all six subcommands smoke-tested via cargo run."
  - task: "6.2"
    commit: null
    fix_up: null
    notes: "DEFERRED by user decision on 2026-05-14. The tool is functionally complete without it — 6.2 is release-automation plumbing (cargo-dist config + GitHub release workflow for prebuilt cross-compiled binaries), not tool functionality. The .github/ hold was never lifted. Reopen by installing cargo-dist, configuring dist metadata for x86_64-gnu / x86_64-musl / aarch64-musl with ci=[\"github\"], and running `dist generate`. Nothing in 6.2 blocks `cargo install --path .` or any subcommand."
deliverable: "Python files deleted, repo build/install driven entirely by cargo. (Release automation — 6.2 — deferred; not required for the tool to build, install, or run.)"
tasks:
  - id: "6.1"
    title: "Delete Python implementation and rewrite repo metadata"
    status: complete
    verification: "After this task: `powercfg.py`, `pyproject.toml`, `powercfg.egg-info/`, and the Python-specific Makefile targets are gone from `git ls-files`. `make build` runs `cargo build --release` and produces `target/release/powercfg`. `make install` runs `cargo install --path .`. `make test` runs `cargo test`. `make clean` runs `cargo clean`. `make lint` runs `cargo clippy --all-targets -- -D warnings`. README install instructions reference `cargo install --path .` and the `cargo install powercfg` command (after release). The Windows powercfg comparison table in the README still appears (preserved verbatim)."
  - id: "6.2"
    title: "GitHub Actions release workflow with cargo-dist"
    status: deferred
    depends_on: ["6.1"]
    verification: "`.github/workflows/release.yml` triggers on tag push matching `v*.*.*`. The workflow builds three artifacts: `x86_64-unknown-linux-musl` (static), `aarch64-unknown-linux-musl` (static), `x86_64-unknown-linux-gnu` (dynamic glibc). Each artifact is a tar.gz containing the `powercfg` binary plus README and LICENSE. A test tag (`v1.0.0-rc1`) on a fork or branch produces all three artifacts attached to a draft release. The CI workflow from Phase 1 still runs on every PR."
---

# Phase 6: Cutover

## Overview

Final phase: delete the Python implementation, rewire repo metadata to cargo, and wire up release automation. After this phase merges, `powercfg.py` exists only in git history.

This is two tasks because they target different concerns: 6.1 is local repo plumbing (Makefile, README, file deletions); 6.2 is CI infrastructure (release workflow). They can land in the same PR if review bandwidth permits.

## 6.1: Delete Python implementation and rewrite repo metadata

### Subtasks
- [x] `git rm powercfg.py pyproject.toml`
- [x] `git rm -r powercfg.egg-info/` if tracked, otherwise just remove from disk and add to `.gitignore` if not already. (Not present in this repo; `__pycache__/` removed from disk.)
- [x] Bump `Cargo.toml` to `version = "2.0.0"` — major version reflects the implementation rewrite for anyone installing from source.
- [x] Audit `.gitignore` and remove Python-specific patterns (`__pycache__/`, `*.py[cod]`, `*.egg-info/`, `dist/`, `venv/`, `build/`, `.pytest_cache/`) that no longer apply. The Rust `target/` pattern was added in task 1.1.
- [x] Rewrite `Makefile`:
  - `build:` → `cargo build --release`
  - `install:` → `cargo install --path .`
  - `install-dev:` → `cargo install --path . --debug`
  - `test:` → `cargo test`
  - `lint:` → `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
  - `clean:` → `cargo clean`
  - Drop the `venv`, `dist`, `upload`, `upload-test`, `clean-build` targets.
- [x] Update `README.md`:
  - Replace "Requirements" section: Rust 1.85+ stable (corrected from doc's 1.78+; edition 2024 needs 1.85+), Linux with systemd.
  - Replace "Installation" section: `cargo install --path .` for source build; `cargo install powercfg` once published; mention prebuilt binaries from GitHub releases.
  - ~~Document the new `--json` global flag with one example output snippet.~~ **Deferred:** Phase 5 not landed; will be added post-Phase 5.
  - Preserve the "Commands" sections, the example output, and the "Windows powercfg Equivalents" table verbatim. (Quality-scan fix-up replaced `./powercfg.py` invocations in those sections with bare `powercfg` to match the cargo-installed binary path.)
- [x] Update `.editorconfig` for Rust files if the existing config doesn't already cover `*.rs` (4-space indent, LF endings).
- [x] Document the shebang/symlink change in the README: the old `chmod +x powercfg.py; ln -s ... ~/.local/bin/powercfg` instructions become `cargo install --path .` (which puts `powercfg` in `~/.cargo/bin/`).
- [x] If a user has the old symlink at `~/.local/bin/powercfg`, they need to remove it manually — call this out in a one-line "Upgrading from the Python version" note.

### Notes
The Python version was 1.0.0; the 2.0.0 bump signals the implementation rewrite to anyone installing from source.

## 6.2: GitHub Actions release workflow with cargo-dist

> **DEFERRED (2026-05-14, user decision).** The tool is functionally
> complete without this task — it is release-automation plumbing, not
> tool functionality. The `.github/` hold was never lifted. None of the
> items below block `cargo install --path .` or any subcommand.

### Subtasks
- [ ] *(deferred)* Run `cargo dist init` and accept its defaults, then customize:
  - Targets: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-unknown-linux-gnu`.
  - Installer: shell installer (curl-pipe-sh) and homebrew formula skipped (Linux-only tool, brew-on-Linux is an edge case).
  - GitHub Actions workflow generated at `.github/workflows/release.yml`.
- [ ] *(deferred)* Add musl cross-compile setup: install `musl-tools` in CI; for aarch64-musl use `cross` (cargo-dist handles this if configured).
- [ ] *(deferred)* Tag `v2.0.0-rc1` on a feature branch, push, verify the release workflow produces three artifacts attached to a draft release. Delete the test release once verified.
- [ ] *(deferred)* Document the release process in `CONTRIBUTING.md` (or append to README): "Tag `vX.Y.Z` on `main`; the release workflow handles the rest."

### Notes
`cargo-dist` regenerates the workflow on each `cargo dist init` run. Don't hand-edit `release.yml` — edit `Cargo.toml`'s `[workspace.metadata.dist]` section and regenerate.

To reopen: install cargo-dist, configure dist metadata for the three
targets with `ci = ["github"]`, run `dist generate`, then perform the
tag-test on a branch. No source changes are required — the binary and
its CLI surface are already final.

## Acceptance Criteria

- [x] `git ls-files` shows no `*.py`, no `pyproject.toml`, no `powercfg.egg-info/`.
- [x] `make build`, `make install`, `make test`, `make lint`, `make clean` all work and drive cargo.
- [x] README install instructions are accurate; an `Upgrading from the Python version` note exists.
- [x] `Cargo.toml` is at version `2.0.0`.
- [ ] *(deferred with 6.2)* A test tag pushed to a branch produces a draft release with three target artifacts.
- [ ] *(deferred — CI workflow held by user)* The standard CI workflow from Phase 1 still runs on every PR.
- [x] `cargo install --path .` from a fresh clone produces a working `powercfg` binary in `~/.cargo/bin/`.
