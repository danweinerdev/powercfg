#!/usr/bin/env bash
# Capture a fresh sysfs/procfs fixture tree from the running machine.
#
# Usage:
#   scripts/capture_fixtures.sh ./tests/fixtures/sys-mymachine
#
# The script copies the sysfs/procfs subtrees the powercfg parsers read,
# preserving directory structure under the target dir. It does NOT
# automatically redact identifiers — see the reminder printed at the end.
#
# Walks: /sys/power, /sys/class/{power_supply,thermal,hwmon,rtc},
# /sys/bus/{usb,pci}/devices, /sys/devices/system/cpu, and
# /proc/{interrupts,swaps,acpi/wakeup}. Files that don't exist on this
# machine are silently skipped.

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <target-fixture-dir>" >&2
    exit 2
fi

TARGET="$1"
mkdir -p "$TARGET"

# Single-file captures from /sys/power.
SYS_POWER_FILES=(
    /sys/power/state
    /sys/power/mem_sleep
    /sys/power/disk
    /sys/power/image_size
    /sys/power/wake_lock
    /sys/power/pm_wakeup_irq
)

# Subtree captures. cp --parents preserves the leading directories under
# TARGET so /sys/class/power_supply/BAT0/capacity lands at
# $TARGET/sys/class/power_supply/BAT0/capacity.
SYS_SUBTREES=(
    /sys/class/power_supply
    /sys/class/thermal
    /sys/class/hwmon
    /sys/class/rtc
    /sys/bus/usb/devices
    /sys/bus/pci/devices
    /sys/devices/system/cpu
)

PROC_FILES=(
    /proc/interrupts
    /proc/swaps
    /proc/acpi/wakeup
)

copy_file() {
    local src="$1"
    if [[ -e "$src" ]]; then
        cp --parents -- "$src" "$TARGET" 2>/dev/null || true
    fi
}

copy_subtree() {
    local src="$1"
    if [[ -d "$src" ]]; then
        # find -type f handles symlink/dir mix in /sys without recursing
        # forever. -L would chase symlinks; we deliberately want the raw
        # files only.
        find "$src" -type f -print0 2>/dev/null \
            | xargs -0 -r -n1 -I{} cp --parents -- "{}" "$TARGET" 2>/dev/null \
            || true
    fi
}

for f in "${SYS_POWER_FILES[@]}"; do
    copy_file "$f"
done

for d in "${SYS_SUBTREES[@]}"; do
    copy_subtree "$d"
done

for f in "${PROC_FILES[@]}"; do
    copy_file "$f"
done

cat <<'EOF'

# Manual redaction needed:
#
# Before committing this fixture, scrub identifiers from the captured
# files. The most likely places to find them:
#
# Text-format identifier files:
#   $TARGET/sys/bus/usb/devices/*/{manufacturer,product,serial}
#   $TARGET/sys/bus/pci/devices/*/{label,*_id}
#   $TARGET/sys/class/power_supply/*/{manufacturer,model_name,serial_number}
#   $TARGET/sys/devices/.../macaddress
#
# Binary blobs that may contain hardware-identifying bytes (MAC ranges,
# device serials, capability strings) — none of these are read by
# powercfg, so the safest action is to delete them outright rather than
# trying to redact in place:
#   $TARGET/sys/bus/pci/devices/*/config           (256+ bytes per device)
#   $TARGET/sys/bus/pci/devices/*/resource[0-9]*   (BAR contents)
#   $TARGET/sys/bus/pci/devices/*/rom              (option ROM dumps)
#   $TARGET/sys/bus/usb/devices/*/descriptors
#
# Quick scrub commands (run from $TARGET):
#   find . -name 'config' -path '*pci/devices*' -delete
#   find . -name 'resource[0-9]*' -path '*pci/devices*' -delete
#   find . -name 'rom' -path '*pci/devices*' -delete
#   find . -name 'descriptors' -path '*usb/devices*' -delete
#
# Then `grep -r '<your name or serial fragment>' .` as a spot-check.
# MAC addresses, machine UUIDs, and BIOS serials have no testing value —
# in text files, replace them with deterministic placeholders (e.g.
# "REDACTED") rather than deleting, so the parsers still exercise the
# read path.
EOF
