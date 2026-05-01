---
title: "JSON Output"
type: phase
plan: RustRewrite
phase: 5
status: planned
created: 2026-04-28
updated: 2026-04-28
deliverable: "`--json` global flag works for all six subcommands; JSON shape locked by snapshot tests."
tasks:
  - id: "5.1"
    title: "Serialize derives and JSON schema documentation"
    status: planned
    verification: "Every public model struct (`RequestsReport`, `LastWakeReport`, `DeviceQueryReport`, `SleepStatesReport`, `WakeTimersReport`, `EnergyReport` and their nested types) carries `#[derive(Serialize)]` with `#[serde(rename_all = \"snake_case\")]` where field names need adjustment. Optional fields use `#[serde(skip_serializing_if = \"Option::is_none\")]` only where the schema says `null` for absent. `docs/json-schema.md` documents one representative JSON object per subcommand, matching the sketches in the design doc."
  - id: "5.2"
    title: "Wire --json global flag with snapshot tests for all six commands"
    status: planned
    depends_on: ["5.1"]
    verification: "Each of the six subcommands run with `--json` against the fixture sysroot produces snapshot-stable JSON. The JSON parses successfully via `serde_json::from_str::<serde_json::Value>` (round-trip). Field names, types, and nullability match the sketches in `Designs/RustRewrite/README.md` Decision 5. The text output mode is unchanged (existing snapshots still pass). Empty collections serialize as `[]`, absent optional fields serialize as `null` (or omit, per the per-command schema)."
---

# Phase 5: JSON Output

## Overview

Adds the only feature the Python tool lacks: structured output via `--json`. Because the command modules already build single owning report structs, this is mostly mechanical — `#[derive(Serialize)]` plus a one-line dispatch in each command handler.

The phase is separated from the per-command phases so the text-mode snapshots stabilize first and one big Serialize-derive sweep lands in a single review-friendly diff.

## 5.1: Serialize derives and JSON schema documentation

### Subtasks
- [ ] Add `serde = { version = "1", features = ["derive"] }` and `serde_json` to `Cargo.toml`.
- [ ] Annotate every model struct with `#[derive(Serialize)]`. Apply `#[serde(rename_all = "snake_case")]` at the struct level so Rust's `cur_freq_khz` matches the schema's `cur_freq_khz` (already snake but consistency).
- [ ] For each `Option<T>` field where the schema says the key may be omitted entirely, add `#[serde(skip_serializing_if = "Option::is_none")]`. For fields where the schema says `null`, leave the default behavior.
- [ ] For `Vec` fields whose presence is conditional on a CLI flag (e.g., `kernel_messages` only with `-v`, `history` only with `-n`), use `Option<Vec<T>>` instead of plain `Vec<T>` so the key is omitted from JSON when the flag is unset. A plain empty `Vec` would serialize as `[]`, which would be ambiguous with "the flag was set but there were no events" — `null`/missing makes the distinction explicit.
- [ ] Apply the same Option-shape pattern at section level: when a source returned `Err` (D-Bus down, sysfs unreadable, subprocess timeout), the corresponding report field is `None` and serializes as `null` in JSON. When the source returned `Ok(empty)`, the field serializes as `[]`. This makes "data unavailable" distinguishable from "data was queryable but empty" — verified by a unit test that constructs a report with one `None` field and one empty-`Vec` field, serializes, and asserts the JSON keys differ in shape.
- [ ] Verify enum serialization: `SleepEventKind::Sleep` should serialize as `"sleep"`, `Wake` as `"wake"` — use `#[serde(rename_all = "lowercase")]` on the enum.
- [ ] Write `docs/json-schema.md`: one representative JSON object per subcommand, copied from the design doc Decision 5 sketches. Add a one-line note: "Removing or renaming a field is a breaking change. Adding optional fields is non-breaking."
- [ ] Round-trip unit test: construct a sample of every report struct with mixed populated/empty fields, serialize, parse back via `serde_json::Value`, assert the expected shape.

### Notes
Don't add `#[derive(Deserialize)]` — there is no consumer of these structs as input today. Adding it later is a non-breaking change.

## 5.2: Wire --json global flag with snapshot tests for all six commands

### Subtasks
- [ ] In `cli.rs`, the existing `--json` global flag dispatches through to a `format` enum: `Format::Text` or `Format::Json`.
- [ ] In each `cmd::*::run`, after building the report struct, branch on the format: `Format::Text => format::text::print_*(...)` or `Format::Json => serde_json::to_writer_pretty(io::stdout(), &report)?; println!()`.
- [ ] `tests/json.rs` — for each of the six subcommands, run `powercfg <cmd> --json` against the fixture sysroot, snapshot the output via `insta::assert_json_snapshot!`. Also include verbose variants where flags differ (`devicequery -v --json`, `lastwake -v --json`, etc.).
- [ ] CLI snapshot for `--help` updated to show `--json` flag (existing snapshot will need acceptance).

### Notes
Use `to_writer_pretty` for human-readability — the output is small (KB at most) and pretty-printing makes it usable directly in a shell. Tools that want compact output can pipe through `jq -c`.

## Acceptance Criteria

- [ ] All six subcommands produce snapshot-stable, valid JSON when run with `--json`.
- [ ] `docs/json-schema.md` exists and documents the shape of each subcommand's output.
- [ ] All existing text-mode snapshots still pass — no regression.
- [ ] JSON output round-trips through `serde_json::from_str::<Value>` without error.
- [ ] `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all green.
