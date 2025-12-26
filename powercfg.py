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

    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        return 1

    return args.func(args)


if __name__ == "__main__":
    exit(main() or 0)
