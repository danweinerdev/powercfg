#!/usr/bin/env python3
"""
Linux equivalent of Windows 'powercfg' commands.
Shows what's preventing the system from sleeping and wake history.
"""

import argparse
import subprocess
import re
from datetime import datetime, timedelta
from pathlib import Path


def parse_iso_timestamp(timestamp_str):
    """Parse ISO timestamp with timezone."""
    # Handle both -08:00 and -0800 formats
    if len(timestamp_str) == 25 and timestamp_str[-3] == ':':
        # Format: 2025-12-25T21:40:21-08:00
        return datetime.strptime(timestamp_str, "%Y-%m-%dT%H:%M:%S%z")
    else:
        # Format: 2025-12-25T21:40:21-0800
        return datetime.strptime(timestamp_str, "%Y-%m-%dT%H:%M:%S%z")


def format_duration(td):
    """Format a timedelta as human-readable duration."""
    total_seconds = int(td.total_seconds())
    days, remainder = divmod(total_seconds, 86400)
    hours, remainder = divmod(remainder, 3600)
    minutes, seconds = divmod(remainder, 60)

    parts = []
    if days > 0:
        parts.append(f"{days}d")
    if hours > 0:
        parts.append(f"{hours}h")
    if minutes > 0:
        parts.append(f"{minutes}m")
    if seconds > 0 or not parts:
        parts.append(f"{seconds}s")

    return " ".join(parts)


def get_systemd_inhibitors():
    """Get sleep inhibitors from systemd-logind."""
    inhibitors = []
    try:
        result = subprocess.run(
            ["systemd-inhibit", "--list", "--no-legend"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0 and result.stdout.strip():
            lines = result.stdout.strip().split('\n')
            for line in lines:
                parts = line.split()
                if len(parts) >= 5:
                    who = parts[0]
                    uid = parts[1]
                    user = parts[2]
                    pid = parts[3]
                    comm = parts[4]
                    what = parts[5] if len(parts) > 5 else "sleep"
                    why = " ".join(parts[6:]) if len(parts) > 6 else "Unknown"
                    inhibitors.append({
                        "who": who,
                        "user": user,
                        "pid": pid,
                        "command": comm,
                        "what": what,
                        "why": why
                    })
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass
    return inhibitors


def get_systemd_inhibitors_dbus():
    """Get sleep inhibitors via D-Bus (alternative method)."""
    inhibitors = []
    try:
        result = subprocess.run(
            ["busctl", "call", "org.freedesktop.login1",
             "/org/freedesktop/login1", "org.freedesktop.login1.Manager",
             "ListInhibitors"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0 and result.stdout.strip():
            # Parse D-Bus output
            output = result.stdout
            # Format: a(ssssuu) N [(what, who, why, mode, uid, pid), ...]
            if "sleep" in output.lower() or "idle" in output.lower():
                # Fallback to systemd-inhibit parsing
                pass
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass
    return inhibitors


def get_kernel_wake_locks():
    """Get wake locks from kernel."""
    wake_locks = []
    wake_lock_path = Path("/sys/power/wake_lock")
    if wake_lock_path.exists():
        try:
            content = wake_lock_path.read_text().strip()
            if content:
                wake_locks = content.split()
        except PermissionError:
            pass
    return wake_locks


def get_audio_status():
    """Check if audio is currently playing."""
    audio_playing = []
    try:
        # Check PulseAudio/PipeWire for running streams
        result = subprocess.run(
            ["pactl", "list", "sink-inputs", "short"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0 and result.stdout.strip():
            lines = result.stdout.strip().split('\n')
            for line in lines:
                if line.strip():
                    parts = line.split('\t')
                    if len(parts) >= 2:
                        audio_playing.append({
                            "id": parts[0],
                            "client": parts[2] if len(parts) > 2 else "Unknown"
                        })
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass
    return audio_playing


def get_usb_wakeup_devices():
    """Get USB devices with wakeup enabled."""
    wakeup_devices = []
    usb_path = Path("/sys/bus/usb/devices")
    if usb_path.exists():
        for device in usb_path.iterdir():
            wakeup_file = device / "power" / "wakeup"
            if wakeup_file.exists():
                try:
                    status = wakeup_file.read_text().strip()
                    if status == "enabled":
                        # Get device info
                        product_file = device / "product"
                        manufacturer_file = device / "manufacturer"
                        product = "Unknown Device"
                        manufacturer = ""
                        if product_file.exists():
                            try:
                                product = product_file.read_text().strip()
                            except (PermissionError, OSError):
                                pass
                        if manufacturer_file.exists():
                            try:
                                manufacturer = manufacturer_file.read_text().strip()
                            except (PermissionError, OSError):
                                pass
                        name = f"{manufacturer} {product}".strip() or device.name
                        wakeup_devices.append({
                            "device": device.name,
                            "name": name
                        })
                except (PermissionError, OSError):
                    pass
    return wakeup_devices


def get_active_network_connections():
    """Check for active network connections that might prevent sleep."""
    connections = []
    try:
        result = subprocess.run(
            ["ss", "-tp", "--no-header"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0:
            lines = result.stdout.strip().split('\n')
            established = [l for l in lines if 'ESTAB' in l]
            if len(established) > 5:
                connections.append({
                    "type": "network",
                    "count": len(established),
                    "description": f"{len(established)} established TCP connections"
                })
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass
    return connections


def get_running_vms():
    """Check for running virtual machines."""
    vms = []
    # Check for QEMU/KVM
    try:
        result = subprocess.run(
            ["pgrep", "-a", "qemu"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0 and result.stdout.strip():
            for line in result.stdout.strip().split('\n'):
                parts = line.split(None, 1)
                if len(parts) >= 2:
                    vms.append({"pid": parts[0], "name": "QEMU/KVM VM"})
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass

    # Check for VirtualBox
    try:
        result = subprocess.run(
            ["pgrep", "-a", "VBoxHeadless"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0 and result.stdout.strip():
            for line in result.stdout.strip().split('\n'):
                parts = line.split(None, 1)
                if len(parts) >= 2:
                    vms.append({"pid": parts[0], "name": "VirtualBox VM"})
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass

    return vms


def get_last_wake_time():
    """Get the last wake time from systemd journal."""
    try:
        # Use pipe through grep for better performance on large journals
        result = subprocess.run(
            "journalctl -k -o short-iso --no-pager --since '7 days ago' | grep 'PM: suspend exit' | tail -1",
            shell=True,
            capture_output=True,
            text=True,
            timeout=15
        )
        if result.returncode == 0 and result.stdout.strip():
            line = result.stdout.strip()
            # Match ISO timestamp with timezone (handles both -0800 and -08:00 formats)
            match = re.match(r'^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:?\d{2})', line)
            if match:
                return match.group(1)
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass
    return None


def get_last_sleep_time():
    """Get the last sleep time from systemd journal."""
    try:
        # Use pipe through grep for better performance on large journals
        result = subprocess.run(
            "journalctl -k -o short-iso --no-pager --since '7 days ago' | grep 'PM: suspend entry' | tail -1",
            shell=True,
            capture_output=True,
            text=True,
            timeout=15
        )
        if result.returncode == 0 and result.stdout.strip():
            line = result.stdout.strip()
            # Match ISO timestamp with timezone (handles both -0800 and -08:00 formats)
            match = re.match(r'^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:?\d{2})', line)
            if match:
                return match.group(1)
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass
    return None


def get_wake_irq():
    """Get the IRQ that caused the last wake."""
    irq_path = Path("/sys/power/pm_wakeup_irq")
    if irq_path.exists():
        try:
            irq = irq_path.read_text().strip()
            if irq:
                return irq
        except (PermissionError, OSError):
            pass
    return None


def get_irq_info(irq_num):
    """Get information about an IRQ number."""
    try:
        proc_interrupts = Path("/proc/interrupts").read_text()
        for line in proc_interrupts.split('\n'):
            if line.strip().startswith(f"{irq_num}:"):
                parts = line.split()
                # Last part(s) typically contain the device name
                if len(parts) >= 2:
                    return " ".join(parts[-2:])
    except (PermissionError, OSError):
        pass
    return None


def get_acpi_wakeup_devices():
    """Get ACPI wake-capable devices."""
    devices = []
    acpi_path = Path("/proc/acpi/wakeup")
    if acpi_path.exists():
        try:
            content = acpi_path.read_text()
            for line in content.strip().split('\n')[1:]:  # Skip header
                parts = line.split()
                if len(parts) >= 3:
                    device = parts[0]
                    state = parts[1]
                    status = parts[2].strip('*')
                    sysfs = parts[3] if len(parts) > 3 else ""
                    devices.append({
                        "device": device,
                        "state": state,
                        "enabled": status == "enabled",
                        "sysfs": sysfs
                    })
        except (PermissionError, OSError):
            pass
    return devices


def get_pci_device_description(pci_addr):
    """Get a description for a PCI device from sysfs."""
    # pci_addr format: "pci:0000:0d:00.3" -> "0000:0d:00.3"
    if pci_addr.startswith("pci:"):
        addr = pci_addr[4:]
    else:
        addr = pci_addr

    device_path = Path(f"/sys/bus/pci/devices/{addr}")
    if not device_path.exists():
        return None

    description_parts = []

    # Try to get class description
    class_file = device_path / "class"
    if class_file.exists():
        try:
            class_code = class_file.read_text().strip()
            # Class codes: 0x0c03xx = USB, 0x0200xx = Ethernet, etc.
            class_map = {
                "0x0c03": "USB Controller",
                "0x0c05": "SMBus Controller",
                "0x0200": "Ethernet Controller",
                "0x0280": "Network Controller",
                "0x0300": "VGA Controller",
                "0x0403": "Audio Device",
                "0x0108": "NVMe Controller",
                "0x0106": "SATA Controller",
                "0x0604": "PCI Bridge",
                "0x0600": "Host Bridge",
                "0x0580": "Memory Controller",
            }
            class_prefix = class_code[:6]
            if class_prefix in class_map:
                description_parts.append(class_map[class_prefix])
        except (PermissionError, OSError):
            pass

    # Try vendor/device for more specific info
    vendor_file = device_path / "vendor"
    device_file = device_path / "device"
    if vendor_file.exists() and device_file.exists():
        try:
            vendor = vendor_file.read_text().strip()
            # Common vendors
            vendor_map = {
                "0x1022": "AMD",
                "0x10de": "NVIDIA",
                "0x8086": "Intel",
                "0x1002": "AMD/ATI",
                "0x14c3": "MediaTek",
                "0x10ec": "Realtek",
            }
            if vendor in vendor_map and not description_parts:
                description_parts.insert(0, vendor_map[vendor])
        except (PermissionError, OSError):
            pass

    return " ".join(description_parts) if description_parts else None


def get_device_wakeup_stats(sysfs_node):
    """Get wakeup statistics for a device."""
    if not sysfs_node:
        return None

    # Convert sysfs node to path
    if sysfs_node.startswith("pci:"):
        addr = sysfs_node[4:]
        base_path = Path(f"/sys/bus/pci/devices/{addr}/power")
    else:
        return None

    if not base_path.exists():
        return None

    stats = {}
    stat_files = ["wakeup_count", "wakeup_active_count", "wakeup_last_time_ms"]
    for stat in stat_files:
        stat_file = base_path / stat
        if stat_file.exists():
            try:
                stats[stat] = stat_file.read_text().strip()
            except (PermissionError, OSError):
                pass

    return stats if stats else None


def cmd_devicequery(args):
    """Handle the 'devicequery' command."""
    print("WAKE-CAPABLE DEVICES")
    print("=" * 50)

    devices = get_acpi_wakeup_devices()

    # Filter if requested
    if args.enabled_only:
        devices = [d for d in devices if d["enabled"]]

    print("\n[ACPI WAKE DEVICES]")
    print("-" * 30)

    if devices:
        # Print header
        print(f"  {'Device':<8} {'State':<6} {'Status':<10} {'Description'}")
        print(f"  {'-'*6:<8} {'-'*5:<6} {'-'*8:<10} {'-'*20}")

        for dev in devices:
            status = "enabled" if dev["enabled"] else "disabled"
            desc = ""
            if dev["sysfs"]:
                pci_desc = get_pci_device_description(dev["sysfs"])
                if pci_desc:
                    desc = pci_desc
                else:
                    desc = dev["sysfs"]

            print(f"  {dev['device']:<8} {dev['state']:<6} {status:<10} {desc}")

            # Show stats in verbose mode
            if args.verbose and dev["sysfs"]:
                stats = get_device_wakeup_stats(dev["sysfs"])
                if stats:
                    if "wakeup_count" in stats and stats["wakeup_count"] != "0":
                        print(f"           Wake count: {stats['wakeup_count']}")
    else:
        print("  No ACPI wake devices found.")

    # USB devices with wakeup capability
    usb_devices = get_usb_wakeup_devices()
    if usb_devices:
        print("\n[USB WAKE DEVICES]")
        print("-" * 30)
        for dev in usb_devices:
            status = "enabled"
            print(f"  {dev['device']:<12} {status:<10} {dev['name']}")

    # Summary
    enabled_count = len([d for d in devices if d["enabled"]]) + len(usb_devices)
    total_count = len(devices) + len(usb_devices)
    print("\n" + "=" * 50)
    print(f"Wake-enabled devices: {enabled_count} of {total_count}")


def get_sleep_states():
    """Get available sleep states from /sys/power/state."""
    states = []
    state_path = Path("/sys/power/state")
    if state_path.exists():
        try:
            content = state_path.read_text().strip()
            states = content.split()
        except (PermissionError, OSError):
            pass
    return states


def get_mem_sleep_modes():
    """Get memory sleep modes and current selection."""
    modes = []
    current = None
    mem_sleep_path = Path("/sys/power/mem_sleep")
    if mem_sleep_path.exists():
        try:
            content = mem_sleep_path.read_text().strip()
            for mode in content.split():
                if mode.startswith("[") and mode.endswith("]"):
                    current = mode[1:-1]
                    modes.append(current)
                else:
                    modes.append(mode)
        except (PermissionError, OSError):
            pass
    return modes, current


def get_disk_modes():
    """Get disk/hibernation modes and current selection."""
    modes = []
    current = None
    disk_path = Path("/sys/power/disk")
    if disk_path.exists():
        try:
            content = disk_path.read_text().strip()
            for mode in content.split():
                if mode.startswith("[") and mode.endswith("]"):
                    current = mode[1:-1]
                    modes.append(current)
                else:
                    modes.append(mode)
        except (PermissionError, OSError):
            pass
    return modes, current


def get_swap_info():
    """Get swap information for hibernation viability."""
    swaps = []
    swap_path = Path("/proc/swaps")
    if swap_path.exists():
        try:
            content = swap_path.read_text().strip()
            lines = content.split('\n')[1:]  # Skip header
            for line in lines:
                parts = line.split()
                if len(parts) >= 3:
                    swaps.append({
                        "device": parts[0],
                        "type": parts[1],
                        "size_kb": int(parts[2]),
                    })
        except (PermissionError, OSError, ValueError):
            pass
    return swaps


def cmd_sleepstates(args):
    """Handle the 'sleepstates' command."""
    print("AVAILABLE SLEEP STATES")
    print("=" * 50)

    # Sleep state descriptions
    state_desc = {
        "freeze": ("Suspend-to-Idle", "S0ix", "Lowest latency, moderate power savings"),
        "mem": ("Suspend-to-RAM", "S3", "Fast resume, good power savings"),
        "disk": ("Hibernation", "S4", "Slowest resume, best power savings"),
        "standby": ("Standby", "S1", "Light sleep, minimal savings"),
    }

    mem_mode_desc = {
        "s2idle": "Suspend-to-Idle (software-driven)",
        "shallow": "Shallow suspend (platform-assisted)",
        "deep": "Suspend-to-RAM (hardware S3)",
    }

    # Available sleep states
    states = get_sleep_states()
    print("\n[SLEEP STATES]")
    print("-" * 30)
    if states:
        for state in states:
            if state in state_desc:
                name, acpi, desc = state_desc[state]
                print(f"  {state:<10} {name} ({acpi})")
                if args.verbose:
                    print(f"             {desc}")
            else:
                print(f"  {state}")
    else:
        print("  Unable to read sleep states")

    # Memory sleep mode
    mem_modes, mem_current = get_mem_sleep_modes()
    print("\n[MEMORY SLEEP MODE]")
    print("-" * 30)
    if mem_current:
        desc = mem_mode_desc.get(mem_current, mem_current)
        print(f"  Current: {mem_current} - {desc}")
    if mem_modes:
        print(f"  Available: {', '.join(mem_modes)}")

    # Hibernation/disk mode
    disk_modes, disk_current = get_disk_modes()
    if "disk" in states:
        print("\n[HIBERNATION MODE]")
        print("-" * 30)
        if disk_current:
            print(f"  Current: {disk_current}")
        if disk_modes and args.verbose:
            print(f"  Available: {', '.join(disk_modes)}")

        # Check swap for hibernation viability
        swaps = get_swap_info()
        if swaps:
            total_swap_mb = sum(s["size_kb"] for s in swaps) // 1024
            print(f"  Swap: {total_swap_mb} MB available")
            for swap in swaps:
                size_gb = swap["size_kb"] / 1024 / 1024
                is_zram = "zram" in swap["device"]
                note = " (may not support hibernation)" if is_zram else ""
                print(f"    {swap['device']}: {size_gb:.1f} GB{note}")
        else:
            print("  Swap: None configured (hibernation unavailable)")

    # Verbose: show image size limit
    if args.verbose:
        image_size_path = Path("/sys/power/image_size")
        if image_size_path.exists():
            try:
                image_size = int(image_size_path.read_text().strip())
                image_size_mb = image_size // (1024 * 1024)
                print(f"\n[HIBERNATION IMAGE]")
                print("-" * 30)
                print(f"  Max image size: {image_size_mb} MB")
            except (PermissionError, OSError, ValueError):
                pass

    print("\n" + "=" * 50)


def get_wake_source_from_dmesg():
    """Try to identify wake source from dmesg."""
    try:
        result = subprocess.run(
            ["dmesg", "--time-format=iso"],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0:
            lines = result.stdout.split('\n')
            wake_lines = []
            for line in reversed(lines):
                lower = line.lower()
                if 'wakeup' in lower or 'wake up' in lower or 'resume' in lower:
                    wake_lines.append(line)
                    if len(wake_lines) >= 5:
                        break
            return list(reversed(wake_lines))
    except (subprocess.TimeoutExpired, FileNotFoundError, PermissionError):
        pass
    return []


def get_sleep_history(count=5):
    """Get recent sleep/wake history from journal."""
    history = []
    try:
        # Use pipe through grep for better performance on large journals
        result = subprocess.run(
            "journalctl -k -o short-iso --no-pager --since '30 days ago' | grep 'PM: suspend e'",
            shell=True,
            capture_output=True,
            text=True,
            timeout=15
        )
        if result.returncode == 0 and result.stdout.strip():
            for line in result.stdout.strip().split('\n'):
                # Match ISO timestamp with timezone (handles both -0800 and -08:00 formats)
                match = re.match(r'^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:?\d{2})', line)
                if match:
                    timestamp = match.group(1)
                    if "suspend exit" in line:
                        history.append({"time": timestamp, "type": "wake"})
                    elif "suspend entry" in line:
                        history.append({"time": timestamp, "type": "sleep"})
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass
    # Return the most recent entries
    return history[-count:]


def cmd_lastwake(args):
    """Handle the 'lastwake' command."""
    print("LAST WAKE INFORMATION")
    print("=" * 50)

    # Last wake time
    wake_time = get_last_wake_time()
    sleep_time = get_last_sleep_time()

    print("\n[LAST SLEEP/WAKE CYCLE]")
    print("-" * 30)
    if sleep_time:
        print(f"  Sleep time: {sleep_time}")
    else:
        print("  Sleep time: Unknown")

    if wake_time:
        print(f"  Wake time:  {wake_time}")
    else:
        print("  Wake time:  Unknown (system may not have slept this boot)")

    # Calculate and display duration
    if sleep_time and wake_time:
        try:
            sleep_dt = parse_iso_timestamp(sleep_time)
            wake_dt = parse_iso_timestamp(wake_time)
            duration = wake_dt - sleep_dt
            if duration.total_seconds() > 0:
                print(f"  Duration:   {format_duration(duration)}")
        except (ValueError, TypeError):
            pass

    # Wake IRQ
    print("\n[WAKE SOURCE]")
    print("-" * 30)
    irq = get_wake_irq()
    if irq:
        irq_info = get_irq_info(irq)
        print(f"  Wake IRQ: {irq}")
        if irq_info:
            print(f"  Device: {irq_info}")
    else:
        print("  Wake IRQ: Not available")

    # Try dmesg for more info
    dmesg_wake = get_wake_source_from_dmesg()
    if dmesg_wake and args.verbose:
        print("\n[KERNEL WAKE MESSAGES]")
        print("-" * 30)
        for line in dmesg_wake:
            # Truncate long lines
            if len(line) > 70:
                line = line[:67] + "..."
            print(f"  {line}")

    # ACPI wakeup devices
    if args.verbose:
        acpi_devices = get_acpi_wakeup_devices()
        enabled_devices = [d for d in acpi_devices if d["enabled"]]
        print("\n[ENABLED ACPI WAKE DEVICES]")
        print("-" * 30)
        if enabled_devices:
            for dev in enabled_devices:
                sysfs = f" ({dev['sysfs']})" if dev['sysfs'] else ""
                print(f"  {dev['device']}: {dev['state']}{sysfs}")
        else:
            print("  None.")

    # Recent history
    if args.history:
        history = get_sleep_history(args.history)
        print(f"\n[RECENT SLEEP/WAKE HISTORY]")
        print("-" * 30)
        if history:
            for event in history:
                print(f"  {event['time']} - {event['type'].upper()}")
        else:
            print("  No sleep/wake events found.")

    print("\n" + "=" * 50)


def print_section(title, items, empty_msg="None."):
    """Print a formatted section."""
    print(f"\n{title}")
    print("-" * len(title))
    if items:
        for item in items:
            if isinstance(item, dict):
                print(f"  {item}")
            else:
                print(f"  {item}")
    else:
        print(f"  {empty_msg}")


def cmd_requests(args):
    """Handle the 'requests' command."""
    print("POWER REQUEST STATUS")
    print("=" * 50)

    # Systemd Inhibitors (main source on modern Linux)
    inhibitors = get_systemd_inhibitors()
    print("\n[SYSTEM INHIBITORS]")
    print("-" * 30)
    if inhibitors:
        for inh in inhibitors:
            blocks = inh.get('what', 'sleep')
            if 'sleep' in blocks.lower() or 'idle' in blocks.lower():
                print(f"  Process: {inh['command']} (PID: {inh['pid']})")
                print(f"    User: {inh['user']}")
                print(f"    Blocks: {inh['what']}")
                print(f"    Reason: {inh['why']}")
                print()
    else:
        print("  None.")

    # Kernel Wake Locks
    wake_locks = get_kernel_wake_locks()
    print("\n[KERNEL WAKE LOCKS]")
    print("-" * 30)
    if wake_locks:
        for lock in wake_locks:
            print(f"  {lock}")
    else:
        print("  None.")

    # Audio Playback
    audio = get_audio_status()
    print("\n[AUDIO STREAMS]")
    print("-" * 30)
    if audio:
        for stream in audio:
            print(f"  Stream ID: {stream['id']}")
            print(f"    Client: {stream['client']}")
    else:
        print("  None.")

    # Running VMs
    vms = get_running_vms()
    print("\n[VIRTUAL MACHINES]")
    print("-" * 30)
    if vms:
        for vm in vms:
            print(f"  {vm['name']} (PID: {vm['pid']})")
    else:
        print("  None.")

    # USB Wakeup Devices (informational)
    if args.verbose:
        wakeup_devices = get_usb_wakeup_devices()
        print("\n[USB WAKEUP DEVICES]")
        print("-" * 30)
        if wakeup_devices:
            for dev in wakeup_devices:
                print(f"  {dev['name']} ({dev['device']})")
        else:
            print("  None.")

    # Summary
    total_blockers = len(inhibitors) + len(wake_locks) + len(audio) + len(vms)
    print("\n" + "=" * 50)
    if total_blockers > 0:
        print(f"Total sleep blockers found: {total_blockers}")
    else:
        print("No active sleep blockers detected.")


def main():
    parser = argparse.ArgumentParser(
        description="Linux power configuration utility (similar to Windows powercfg)",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s requests            Show what's preventing sleep
  %(prog)s requests -v         Show with additional details
  %(prog)s lastwake            Show last wake information
  %(prog)s lastwake -v         Show with ACPI devices and kernel messages
  %(prog)s lastwake -n 10      Show last 10 sleep/wake events
  %(prog)s devicequery         Show devices that can wake the system
  %(prog)s devicequery -v      Show with wakeup statistics
  %(prog)s sleepstates         Show available sleep states
  %(prog)s sleepstates -v      Show with hibernation details
        """
    )

    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # 'requests' subcommand
    requests_parser = subparsers.add_parser(
        "requests",
        help="Display power requests (what's preventing sleep)"
    )
    requests_parser.add_argument(
        "-v", "--verbose",
        action="store_true",
        help="Show additional information"
    )
    requests_parser.set_defaults(func=cmd_requests)

    # 'lastwake' subcommand
    lastwake_parser = subparsers.add_parser(
        "lastwake",
        help="Display information about the last wake event"
    )
    lastwake_parser.add_argument(
        "-v", "--verbose",
        action="store_true",
        help="Show additional information (ACPI devices, kernel messages)"
    )
    lastwake_parser.add_argument(
        "-n", "--history",
        type=int,
        metavar="N",
        help="Show last N sleep/wake events"
    )
    lastwake_parser.set_defaults(func=cmd_lastwake)

    # 'devicequery' subcommand
    devicequery_parser = subparsers.add_parser(
        "devicequery",
        help="Display devices that can wake the system"
    )
    devicequery_parser.add_argument(
        "-v", "--verbose",
        action="store_true",
        help="Show wakeup statistics for devices"
    )
    devicequery_parser.add_argument(
        "--enabled-only",
        action="store_true",
        help="Only show devices with wakeup enabled"
    )
    devicequery_parser.set_defaults(func=cmd_devicequery)

    # 'sleepstates' subcommand
    sleepstates_parser = subparsers.add_parser(
        "sleepstates",
        help="Display available sleep states and configuration"
    )
    sleepstates_parser.add_argument(
        "-v", "--verbose",
        action="store_true",
        help="Show additional details"
    )
    sleepstates_parser.set_defaults(func=cmd_sleepstates)

    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        return 1

    return args.func(args)


if __name__ == "__main__":
    exit(main() or 0)
