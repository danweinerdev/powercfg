# powercfg

A Linux equivalent of Windows' `powercfg` command. Query power management status, sleep blockers, wake sources, and energy information.

## Requirements

- Rust 1.85+ (stable; needed for the 2024 edition)
- Linux with systemd

## Installation

Build and install from source:

```bash
cargo install --path .
```

This puts the `powercfg` binary in `~/.cargo/bin/`. Make sure that directory is on your `PATH`.

Once published to crates.io, you will also be able to run:

```bash
cargo install powercfg
```

Prebuilt binaries for common Linux targets will be attached to GitHub releases (available after the release workflow lands).

### Upgrading from the Python version

If you previously symlinked `~/.local/bin/powercfg -> powercfg.py`, remove that symlink (`rm ~/.local/bin/powercfg`) so the new binary in `~/.cargo/bin/powercfg` is picked up instead.

## Commands

### requests

Show what's preventing the system from sleeping.

```bash
powercfg requests
powercfg requests -v    # Include USB wakeup devices
```

**Sources:** systemd-inhibit, /sys/power/wake_lock, PulseAudio/PipeWire, running VMs

### lastwake

Show information about the last sleep/wake cycle.

```bash
powercfg lastwake
powercfg lastwake -v        # Include ACPI devices, kernel messages
powercfg lastwake -n 10     # Show last 10 sleep/wake events
```

**Sources:** journalctl, /sys/power/pm_wakeup_irq, /proc/acpi/wakeup

### devicequery

List devices capable of waking the system from sleep.

```bash
powercfg devicequery
powercfg devicequery -v             # Show wake statistics
powercfg devicequery --enabled-only # Only enabled devices
```

**Sources:** /proc/acpi/wakeup, /sys/bus/usb/devices, /sys/bus/pci/devices

### sleepstates

Show available sleep states and current configuration.

```bash
powercfg sleepstates
powercfg sleepstates -v    # Show hibernation modes, image size
```

**Sources:** /sys/power/state, /sys/power/mem_sleep, /sys/power/disk, /proc/swaps

### waketimers

Show scheduled timers that can wake the system.

```bash
powercfg waketimers
powercfg waketimers -v    # Show all timers with wake status
```

**Sources:** systemctl list-timers, /sys/class/rtc/rtc0/wakealarm

### energy

Show power consumption and thermal status.

```bash
powercfg energy
powercfg energy -v    # Show available EPP modes
```

**Sources:** /sys/class/power_supply, /sys/devices/system/cpu/cpufreq, /sys/class/hwmon

## Example Output

```
$ powercfg requests
POWER REQUEST STATUS
==================================================

[SYSTEM INHIBITORS]
------------------------------
  Process: firefox (PID: 12345)
    User: user
    Blocks: sleep
    Reason: Playing audio

[AUDIO STREAMS]
------------------------------
  Stream ID: 42
    Client: Firefox

==================================================
Total sleep blockers found: 2
```

## Windows powercfg Equivalents

| Windows | Linux (this tool) |
|---------|-------------------|
| `powercfg /requests` | `powercfg requests` |
| `powercfg /lastwake` | `powercfg lastwake` |
| `powercfg /devicequery wake_armed` | `powercfg devicequery --enabled-only` |
| `powercfg /availablesleepstates` | `powercfg sleepstates` |
| `powercfg /waketimers` | `powercfg waketimers` |
| `powercfg /energy` | `powercfg energy` |

## License

MIT
