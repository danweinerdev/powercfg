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
#   $TARGET/sys/bus/usb/devices/*/{manufacturer,product,serial}
#   $TARGET/sys/bus/pci/devices/*/{label,*_id}
#   $TARGET/sys/class/power_supply/*/{manufacturer,model_name,serial_number}
#   $TARGET/sys/devices/.../macaddress
#
# A quick `grep -r '<your name or serial fragment>' $TARGET` is a
# reasonable spot-check. MAC addresses, machine UUIDs, and BIOS serials
# have no testing value — replace them with deterministic placeholders
# (e.g. "REDACTED") rather than deleting the files, so the parsers still
# exercise the read path.
EOF
