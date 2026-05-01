---
title: "Cutover"
type: phase
plan: RustRewrite
phase: 6
status: planned
created: 2026-04-28
updated: 2026-04-28
deliverable: "Python files deleted, repo build/install/release driven entirely by cargo."
tasks:
  - id: "6.1"
    title: "Delete Python implementation and rewrite repo metadata"
    status: planned
    verification: "After this task: `powercfg.py`, `pyproject.toml`, `powercfg.egg-info/`, and the Python-specific Makefile targets are gone from `git ls-files`. `make build` runs `cargo build --release` and produces `target/release/powercfg`. `make install` runs `cargo install --path .`. `make test` runs `cargo test`. `make clean` runs `cargo clean`. `make lint` runs `cargo clippy --all-targets -- -D warnings`. README install instructions reference `cargo install --path .` and the `cargo install powercfg` command (after release). The Windows powercfg comparison table in the README still appears (preserved verbatim)."
  - id: "6.2"
    title: "GitHub Actions release workflow with cargo-dist"
    status: planned
    depends_on: ["6.1"]
    verification: "`.github/workflows/release.yml` triggers on tag push matching `v*.*.*`. The workflow builds three artifacts: `x86_64-unknown-linux-musl` (static), `aarch64-unknown-linux-musl` (static), `x86_64-unknown-linux-gnu` (dynamic glibc). Each artifact is a tar.gz containing the `powercfg` binary plus README and LICENSE. A test tag (`v1.0.0-rc1`) on a fork or branch produces all three artifacts attached to a draft release. The CI workflow from Phase 1 still runs on every PR."
---

# Phase 6: Cutover

## Overview

Final phase: delete the Python implementation, rewire repo metadata to cargo, and wire up release automation. After this phase merges, `powercfg.py` exists only in git history.

This is two tasks because they target different concerns: 6.1 is local repo plumbing (Makefile, README, file deletions); 6.2 is CI infrastructure (release workflow). They can land in the same PR if review bandwidth permits.

## 6.1: Delete Python implementation and rewrite repo metadata

### Subtasks
- [ ] `git rm powercfg.py pyproject.toml`
- [ ] `git rm -r powercfg.egg-info/` if tracked, otherwise just remove from disk and add to `.gitignore` if not already.
- [ ] Bump `Cargo.toml` to `version = "2.0.0"` — major version reflects the implementation rewrite for anyone installing from source.
- [ ] Audit `.gitignore` and remove Python-specific patterns (`__pycache__/`, `*.py[cod]`, `*.egg-info/`, `dist/`, `venv/`, `build/`, `.pytest_cache/`) that no longer apply. The Rust `target/` pattern was added in task 1.1.
- [ ] Rewrite `Makefile`:
  - `build:` → `cargo build --release`
  - `install:` → `cargo install --path .`
  - `install-dev:` → `cargo install --path . --debug`
  - `test:` → `cargo test`
  - `lint:` → `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
  - `clean:` → `cargo clean`
  - Drop the `venv`, `dist`, `upload`, `upload-test`, `clean-build` targets.
- [ ] Update `README.md`:
  - Replace "Requirements" section: Rust 1.78+ stable, Linux with systemd.
  - Replace "Installation" section: `cargo install --path .` for source build; `cargo install powercfg` once published; mention prebuilt binaries from GitHub releases.
  - Document the new `--json` global flag with one example output snippet.
  - Preserve the "Commands" sections, the example output, and the "Windows powercfg Equivalents" table verbatim.
- [ ] Update `.editorconfig` for Rust files if the existing config doesn't already cover `*.rs` (4-space indent, LF endings).
- [ ] Document the shebang/symlink change in the README: the old `chmod +x powercfg.py; ln -s ... ~/.local/bin/powercfg` instructions become `cargo install --path .` (which puts `powercfg` in `~/.cargo/bin/`).
- [ ] If a user has the old symlink at `~/.local/bin/powercfg`, they need to remove it manually — call this out in a one-line "Upgrading from the Python version" note.

### Notes
The Python version was 1.0.0; the 2.0.0 bump signals the implementation rewrite to anyone installing from source.

## 6.2: GitHub Actions release workflow with cargo-dist

### Subtasks
- [ ] Run `cargo dist init` and accept its defaults, then customize:
  - Targets: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-unknown-linux-gnu`.
  - Installer: shell installer (curl-pipe-sh) and homebrew formula skipped (Linux-only tool, brew-on-Linux is an edge case).
  - GitHub Actions workflow generated at `.github/workflows/release.yml`.
- [ ] Add musl cross-compile setup: install `musl-tools` in CI; for aarch64-musl use `cross` (cargo-dist handles this if configured).
- [ ] Tag `v2.0.0-rc1` on a feature branch, push, verify the release workflow produces three artifacts attached to a draft release. Delete the test release once verified.
- [ ] Document the release process in `CONTRIBUTING.md` (or append to README): "Tag `vX.Y.Z` on `main`; the release workflow handles the rest."

### Notes
`cargo-dist` regenerates the workflow on each `cargo dist init` run. Don't hand-edit `release.yml` — edit `Cargo.toml`'s `[workspace.metadata.dist]` section and regenerate.

## Acceptance Criteria

- [ ] `git ls-files` shows no `*.py`, no `pyproject.toml`, no `powercfg.egg-info/`.
- [ ] `make build`, `make install`, `make test`, `make lint`, `make clean` all work and drive cargo.
- [ ] README install instructions are accurate; an `Upgrading from the Python version` note exists.
- [ ] `Cargo.toml` is at version `2.0.0`.
- [ ] A test tag pushed to a branch produces a draft release with three target artifacts.
- [ ] The standard CI workflow from Phase 1 still runs on every PR.
- [ ] `cargo install --path .` from a fresh clone produces a working `powercfg` binary in `~/.cargo/bin/`.
