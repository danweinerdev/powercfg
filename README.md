# powercfg

A Linux equivalent of Windows' `powercfg` command. Query power management status, sleep blockers, wake sources, and energy information.

## Requirements

- Python 3.6+
- Linux with systemd
- No external dependencies (uses only standard library)

## Installation

```bash
chmod +x powercfg.py
# Optionally symlink to PATH
ln -s $(pwd)/powercfg.py ~/.local/bin/powercfg
```

## Commands

### requests

Show what's preventing the system from sleeping.

```bash
./powercfg.py requests
./powercfg.py requests -v    # Include USB wakeup devices
```

**Sources:** systemd-inhibit, /sys/power/wake_lock, PulseAudio/PipeWire, running VMs

### lastwake

Show information about the last sleep/wake cycle.

```bash
./powercfg.py lastwake
./powercfg.py lastwake -v        # Include ACPI devices, kernel messages
./powercfg.py lastwake -n 10     # Show last 10 sleep/wake events
```

**Sources:** journalctl, /sys/power/pm_wakeup_irq, /proc/acpi/wakeup

### devicequery

List devices capable of waking the system from sleep.

```bash
./powercfg.py devicequery
./powercfg.py devicequery -v             # Show wake statistics
./powercfg.py devicequery --enabled-only # Only enabled devices
```

**Sources:** /proc/acpi/wakeup, /sys/bus/usb/devices, /sys/bus/pci/devices

### sleepstates

Show available sleep states and current configuration.

```bash
./powercfg.py sleepstates
./powercfg.py sleepstates -v    # Show hibernation modes, image size
```

**Sources:** /sys/power/state, /sys/power/mem_sleep, /sys/power/disk, /proc/swaps

### waketimers

Show scheduled timers that can wake the system.

```bash
./powercfg.py waketimers
./powercfg.py waketimers -v    # Show all timers with wake status
```

**Sources:** systemctl list-timers, /sys/class/rtc/rtc0/wakealarm

### energy

Show power consumption and thermal status.

```bash
./powercfg.py energy
./powercfg.py energy -v    # Show available EPP modes
```

**Sources:** /sys/class/power_supply, /sys/devices/system/cpu/cpufreq, /sys/class/hwmon

## Example Output

```
$ ./powercfg.py requests
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
| `powercfg /requests` | `./powercfg.py requests` |
| `powercfg /lastwake` | `./powercfg.py lastwake` |
| `powercfg /devicequery wake_armed` | `./powercfg.py devicequery --enabled-only` |
| `powercfg /availablesleepstates` | `./powercfg.py sleepstates` |
| `powercfg /waketimers` | `./powercfg.py waketimers` |
| `powercfg /energy` | `./powercfg.py energy` |

## License

MIT
