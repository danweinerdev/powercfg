# `--json` Output Schema

> Removing or renaming a field is a breaking change. Adding optional fields is non-breaking.

This document fixes the JSON shape `powercfg <subcommand> --json` emits. All
six subcommands print one top-level JSON object. Output is pretty-printed by
default; pipe through `jq -c` for compact output.

## Conventions

- All field names are `snake_case`.
- Empty collections serialize as `[]` (queryable but found none).
- An `Option`-typed scalar field that is `None` serializes as `null`,
  preserving the "data unavailable" signal versus an empty array.
- A handful of `Option<Vec<_>>` fields are *omitted entirely* from the
  JSON when the controlling CLI flag (`--verbose`, `-n`) was not set.
  Each such field is called out below.
- `SleepEventKind` (the `type` field on `lastwake.history` rows)
  serializes as the lowercase string `"sleep"` or `"wake"`.

## `powercfg requests --json`

```jsonc
{
  "inhibitors": [
    {
      "who": "GNOME Settings Daemon",
      "why": "Playing audio",
      "what": "sleep:idle",
      "mode": "block",
      "uid": 1000,
      "pid": 1234,
      "comm": "gsd-power"
    }
  ],
  "wake_locks": ["string", "..."],
  "audio_streams": [
    {
      "id": "42",
      "application_name": "Firefox",      // null when property absent
      "pid": 1234,                         // null when property absent
      "binary": "firefox"                  // null when property absent
    }
  ],
  "vms": [{ "pid": 1234, "comm": "qemu-system-x86" }],
  "usb_wakeup": [{ "device": "1-2", "name": "Logitech Receiver" }]
}
```

Every key is always present. Empty collections serialize as `[]`.

## `powercfg lastwake --json`

```jsonc
{
  "last_sleep": "2025-12-25T21:40:21-08:00",   // null if no recent sleep
  "last_wake":  "2025-12-25T22:10:33-08:00",   // null if no recent wake
  "wake_irq": { "irq": "9", "device": "acpi" },// null if unavailable
  "kernel_messages": ["string", "..."],        // omitted unless --verbose
  "acpi_enabled_devices": [                    // omitted unless --verbose
    { "device": "GPP0", "state": "S4", "enabled": true,
      "sysfs": "pci:0000:00:01.1" }            // sysfs may be null
  ],
  "history": [                                 // omitted unless -n N
    { "time": "2025-12-25T21:40:21-08:00", "type": "sleep" }
  ]
}
```

Deviations from the design-doc sketch:

- `duration_seconds` is **not** a top-level field. The text printer
  computes the wake-minus-sleep duration on render; consumers can do
  the same arithmetic on `last_sleep` and `last_wake`.

## `powercfg devicequery --json`

```jsonc
{
  "acpi_devices": [
    { "device": "GPP0", "state": "S4", "enabled": true,
      "sysfs": "pci:0000:00:01.1" }            // sysfs may be null
  ],
  "usb_devices": [{ "device": "1-2", "name": "Logitech Receiver" }]
}
```

Deviations from the design-doc sketch:

- The schema's per-device `description` and `stats` keys are **not**
  emitted. The text printer reaches back into sysfs to render the
  human-readable PCI description and per-device wakeup counters; that
  data is not stored on the model and is not surfaced in JSON.
- The schema's top-level `totals` object is **not** emitted. The text
  printer derives the enabled-of-total count at render time;
  consumers can compute `enabled = sum(d.enabled for d in acpi_devices)`
  and `total = len(acpi_devices)` from the JSON output.

## `powercfg sleepstates --json`

```jsonc
{
  "states": ["freeze", "mem", "disk"],
  "mem_modes": ["s2idle", "deep"],
  "mem_current": "deep",                       // null if unavailable
  "disk_modes": ["platform", "shutdown", "reboot"],
  "disk_current": "platform",                  // null if unavailable
  "swaps": [
    { "device": "/dev/dm-0", "type": "partition", "size_kb": 16777216 }
  ],
  "image_size_bytes": 4294967296               // omitted unless --verbose
}
```

Deviations from the design-doc sketch:

- The output is **flat**, not nested. The schema sketch shows
  `mem_sleep` and `hibernation` sub-objects; the actual output flattens
  these into top-level `mem_modes` / `mem_current` and `disk_modes` /
  `disk_current` / `swaps` / `image_size_bytes` keys. Consumers wanting
  the nested shape can construct it locally.
- The schema sketch shows `swap[*].size_mb`; the actual key is
  `size_kb` (raw kilobytes from `/proc/swaps`, no unit conversion).
- The schema sketch shows `swap[*].is_zram`; this field is not emitted.
  Consumers can detect zram by checking whether `device` contains
  `"zram"`.
- The schema sketch shows `image_size_mb`; the actual key is
  `image_size_bytes` (raw bytes from `/sys/power/image_size`).

## `powercfg waketimers --json`

```jsonc
{
  "wake_timers": [
    { "unit": "snapshot.timer", "wake_system": true,
      "next_elapse_realtime_us": 1745939400000000 }
  ],
  "all_timers": [                              // omitted unless --verbose
    { "unit": "apt-daily.timer", "wake_system": false,
      "next_elapse_realtime_us": 0 }
  ],
  "rtc_wakealarm": "2026-04-29 06:00:00"       // null if no alarm
}
```

Deviations from the design-doc sketch:

- The schema shows a `next` string field on each timer (already-
  formatted local-time render). The actual output exposes
  `next_elapse_realtime_us`: the raw `NextElapseUSecRealtime`
  property, microseconds since the Unix epoch. Sentinels: `0` =
  unscheduled, `18446744073709551615` (`u64::MAX`) = no next.
  Consumers wanting the human-readable rendering can convert to
  local time themselves; the text printer does this via
  `chrono::Local`.
- The schema shows `wakes` (boolean) for `all_timers` rows; the
  actual key is `wake_system` on every row, matching the systemd
  property name.

## `powercfg energy --json`

```jsonc
{
  "supplies": [
    {
      "name": "BAT0",
      "type": "Battery",                       // null when missing
      "status": "Discharging",                 // null when missing
      "capacity_pct": 87,                      // null when missing
      "level": "Normal",                       // null when missing
      "power_uw": 12500000                     // null when missing
    }
  ],
  "cpu": {
    "driver": "amd-pstate-epp",                // null when missing
    "governor": "powersave",                   // null when missing
    "cur_freq_khz": 3400000,                   // null when missing
    "min_freq_khz": 400000,                    // null when missing
    "max_freq_khz": 4800000,                   // null when missing
    "epp": "balance_performance",              // null when missing
    "epp_available": "performance balance_performance balance_power power",
    "cpu_count": 16
  },
  "temperatures": [
    { "label": "Tctl", "temp_c": 52.4, "source": "k10temp" }
  ],
  "throttle": { "throttle_count": 0 }
}
```

Deviations from the design-doc sketch:

- `throttle.throttled` is **not** emitted. The Python tool's
  `throttled` boolean was derived from `thermal_zone*/mode ==
  "disabled"`, which is not what that ABI means (it indicates a
  zone administratively turned off, not active throttling). Phase 2
  dropped the field; only `throttle_count` (a meaningful historical
  signal) remains.
- The schema sketch shows `cpu.epp_available` as an array of strings;
  the actual output is the raw space-separated string from
  `/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_available_preferences`
  (no split). Consumers can split on whitespace to get the array.

## Round-trip guarantee

The JSON output round-trips through `serde_json::from_str::<Value>`
without error and is unaffected by locale or runtime timezone. Every
report struct has a unit test in its model module proving the shape
contract.
