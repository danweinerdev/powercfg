---
title: "Sysfs Commands"
type: phase
plan: RustRewrite
phase: 2
status: complete
created: 2026-04-28
updated: 2026-04-30
deliverable: "`devicequery` and `energy` shipped end-to-end with fixture-based integration tests."
tasks:
  - id: "2.1"
    title: "ACPI wakeup parsing and PCI device descriptor lookup"
    status: complete
    verification: "Unit tests for `parse_acpi_wakeup` cover: a header-only file, a file with `*enabled` and `enabled` markers, missing-sysfs-column rows, and unrecognized device states. Unit tests for `pci_device_description` cover: a known vendor + class match (AMD USB controller → `AMD USB Controller`), a vendor-only match (NVIDIA → `NVIDIA`), an unknown vendor with known class, and a totally unknown device returning `None`. Test against real captured `/proc/acpi/wakeup` text from both fixture profiles."
  - id: "2.2"
    title: "USB wakeup walking and cmd::devicequery"
    status: complete
    depends_on: ["2.1"]
    verification: "`powercfg devicequery`, `powercfg devicequery -v`, and `powercfg devicequery --enabled-only` all exit 0 against `sys-typical` and produce snapshot-stable output. Verbose mode displays wake-count **only when `wakeup_count` is non-zero** (matches Python's `if stats['wakeup_count'] != '0'` guard on line 464). The summary line reads `Wake-enabled devices: X of Y` where X = enabled-ACPI count + total-USB count (USB entries are by definition enabled — there is no disabled USB row) and Y = total-ACPI count + total-USB count, matching Python lines 479-480. The enabled-only snapshot omits disabled ACPI devices from the table but preserves the summary formula. Live run on the dev machine produces output that matches the Python tool's content (modulo whitespace) for the same machine state."
  - id: "2.3"
    title: "Power supplies, CPU frequency, hwmon, thermal, throttle sources"
    status: complete
    verification: "Unit tests cover: `read_power_supplies` correctly reading capacity/status/power_now/level for both AC and battery entries; `read_cpu_freq_info` extracting driver/governor/cur_freq/min/max/EPP; `read_thermal_info` finding k10temp and coretemp by name and skipping unrelated hwmon entries; `read_throttle_status` accumulating per-CPU package_throttle_count; missing-files paths returning `None`/empty rather than panicking."
  - id: "2.4"
    title: "cmd::energy with sys-headless fixture coverage"
    status: complete
    depends_on: ["2.3"]
    verification: "`powercfg energy` and `powercfg energy -v` produce snapshot-stable output against both `sys-typical` (battery + thermal) and `sys-headless` (no battery, no thermal — the `No power supplies detected (desktop system)` and missing-temperatures branches both fire). Live run on dev machine exits 0. CPU frequency line shows current frequency in human-readable form via `format_freq`."
---

# Phase 2: Sysfs Commands

## Overview

Implements the two remaining sysfs-only commands: `devicequery` (ACPI/USB/PCI wake devices) and `energy` (power supplies, CPU frequency, thermals). After this phase, half of the subcommands are live and the sysfs surface is fully covered. The subprocess-heavy commands in Phase 3 inherit the source-layer patterns established here.

This phase has no new infrastructure work — `SysRoot`, format helpers, snapshot-test machinery all came in Phase 1. It is mostly parsing and printing.

## 2.1: ACPI wakeup parsing and PCI device descriptor lookup

### Subtasks
- [x] `source::procfs::parse_acpi_wakeup(&str) -> Result<Vec<AcpiWakeDevice>, SourceError>` — parses textual `/proc/acpi/wakeup`. Header skip, `*enabled`/`enabled`/`*disabled` markers, optional sysfs column. Header-only → `Ok(vec![])`; non-empty body with all-malformed rows → `Err(Parse)`.
- [x] `source::sysfs::read_pci_device_description` — class+vendor lookup against Python's tables. Composition: vendor + class when both match (`"AMD USB Controller"`); otherwise the matching label alone; neither → `Ok(None)`. Returns `SourceError::NotFound` when the device dir is absent (caller maps to skip-this-device).
- [x] `source::sysfs::read_pci_wakeup_stats` — atomic three-counter read. Stricter than Python (any missing → `Err(Io)`); display path only consumes `wakeup_count` so the stricter behavior is benign.
- [x] `model::devicequery::{AcpiWakeDevice, WakeupStats, UsbWakeDevice, DeviceQueryReport}` — owning structs. `UsbWakeDevice` and the report's `usb_devices` field declared now for shape stability; populated by 2.2.
- [x] Unit + fixture-tree tests: 21 added at initial commit; 2 more fixture-tree tests added in the quality fix-up to lock the committed sys-typical PCI device against the reader.

### Notes
The PCI vendor/class maps live in `match` expressions in `sysfs.rs` rather than a shared module — only used at this call site.

Initial implementation at commit `22c9580`. Quality fix-ups at `0590c48`: stronger fixture-tree assertions, two new PCI fixture-tree tests, dropped redundant `#[allow(dead_code)]` from four private helpers (rustc propagates the lint from the public callers).

Test count: 68 → 91 (+23).

## 2.2: USB wakeup walking and cmd::devicequery

### Subtasks
- [x] `source::sysfs::read_usb_wakeup_devices` — walks `/sys/bus/usb/devices/`, includes `enabled` entries, joins manufacturer + product (or falls back to dir name). Missing parent → `Ok(vec![])`.
- [x] `cmd::devicequery::run` — wires `read_acpi_wakeup` + `read_usb_wakeup_devices`, hands `DeviceQueryReport` + `&SysRoot` to printer. `tracing::debug!` on per-source errors.
- [x] `format::text::print_devicequery` — 4-column ACPI table, optional `[USB WAKE DEVICES]` section (suppressed if empty), optional verbose wake-count line (only if `wakeup_count > 0`), summary footer always computed from the **unfiltered** ACPI list.
- [x] Fixture extensions: 3 USB devices under `sys-typical` (1-2 enabled with metadata, 1-3 disabled, 2-1 enabled without metadata to exercise the dir-name fallback).
- [x] `tests/devicequery.rs` — 4 snapshot tests (typical, typical -v, typical --enabled-only, headless).
- [x] Item-level `#[allow(dead_code)]` annotations dropped on every type/function this task wired through.

### Notes
The Python tool writes the table with fixed-width columns; preserved verbatim.

Initial implementation at commit `c1800da`. Quality fix-ups at `6c7680f`: clarify the `enabled_only` docstring (Python's mechanism vs Rust's), add a Unix symlink test for `read_usb_wakeup_devices` covering the kernel's `usb1`/`usb2` alias pattern.

The `print_devicequery` printer reaches back into `source::sysfs` for per-row PCI enrichment (description + wakeup stats). This is a tracked deferred-refactor — revisit if a third printer needs the same pattern; for now it's contained.

Test count: 91 → 102 (+11).

## 2.3: Power supplies, CPU frequency, hwmon, thermal, throttle sources

### Subtasks
- [x] `source::sysfs::read_power_supplies` — walks `/sys/class/power_supply/`, reads each field independently, returns `Ok(vec![])` when the parent dir is absent. Sorted case-insensitively for snapshot stability.
- [x] `source::sysfs::read_cpu_freq_info` — `Ok` always; missing cpufreq dir returns `Default { cpu_count: N }`. EPP fields are `None` on older kernels.
- [x] `source::sysfs::read_thermal_info` — filters hwmon to `{k10temp, coretemp, zenpower}`, pairs `tempN_input` with `tempN_label` (or `"CPU"` fallback). Sensors sorted numerically (temp2 before temp10).
- [x] `source::sysfs::read_throttle_status` — sums `cpu*/thermal_throttle/package_throttle_count`. **Diverges from Python**: drops the `thermal_zone*/mode` walk and the `throttled: bool` field — Python conflated `mode=disabled` (zone administratively turned off) with "CPU currently throttled" which is wrong on every healthy system. The 2.4 printer will rely on `throttle_count > 0` as the only meaningful signal.
- [x] `model::energy::{PowerSupply, CpuFreqInfo, ThermalReading, ThrottleStatus, EnergyReport}` — owning structs. `ThrottleStatus` no longer carries `throttled` (see above).
- [x] 26 unit tests + 4 fixture-tree integration tests against the committed `sys-typical` fixtures.

### Notes
CPU-name match: `name.starts_with("cpu") && name[3..].chars().all(is_ascii_digit)` — regex-free, matches Python's startswith+isdigit.

Initial implementation at commit `afd1944`. Quality fix-ups at `62785dc`: drop `throttled` (Python bug), numeric tempN sort, case-insensitive power supply sort, fixture-tree integration tests.

Test count: 102 → 129 (+27).

## 2.4: cmd::energy with sys-headless fixture coverage

### Subtasks
- [x] `cmd::energy::run` — wires four sysfs readers, builds `EnergyReport`, hands to printer.
- [x] `format::text::print_energy` — four sections: power supplies (with `No power supplies detected (desktop system)` fallback), CPU frequency (always-printed header, conditional per-field lines, verbose `Available:` EPP line), temperatures (suppressed when empty), thermal throttling (suppressed unless `throttle_count > 0`).
- [x] Fixtures already from 2.3 sufficient — no extension needed.
- [x] `tests/energy.rs` — 3 snapshot tests (typical, typical -v, headless-loop covering both `-v` and no-`-v` against the same snapshot since they're byte-identical when EPP is absent).
- [x] Item-level allows dropped on 10 items wired by this task; 2 new field-level allows added on `PowerSupply::kind` and `ThermalReading::source` (consumed by Phase 5 JSON, unused by text printer).

### Notes
**Diverges from Python**: drops the `Status: THROTTLED/Not throttled` line entirely (per the 2.3 fix). The `[THERMAL THROTTLING]` section now only appears when `throttle_count > 0` — a system that's never thermally throttled produces no section at all.

Initial implementation at commit `6bb9e77`. Quality fix-ups at `bd8b63d`: make `EnergyReport.cpu` non-optional (eliminates an unreachable empty-section bug) and fold the redundant `energy_headless_verbose` test into a 2-iteration loop in `energy_headless`.

Test count: 129 → 132 (+3 net: +4 added, -1 redundant dropped at quality pass).

## Acceptance Criteria

- [x] `powercfg devicequery`, `devicequery -v`, `devicequery --enabled-only` all exit 0 and produce snapshot-stable output against both fixture profiles.
- [x] `powercfg energy`, `energy -v` exit 0 and produce snapshot-stable output against both fixture profiles.
- [x] Live runs on the dev machine produce content matching the Python tool for the same machine state.
- [x] `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all green.
- [x] `lastwake`, `requests`, `waketimers` stubs still panic cleanly when invoked (no regression of Phase 1's stub behavior).
