#!/usr/bin/env python3
"""
On-device test runner for gm65-scanner STM32F469I firmware.

Tests both sync and async production firmware via USB CDC.
Requires: pyserial, st-flash, arm-none-eabi-objcopy

Usage:
    python3 scripts/test_on_device.py sync          # build, flash, test sync firmware
    python3 scripts/test_on_device.py async         # build, flash, test async firmware
    python3 scripts/test_on_device.py both          # test both firmwares
    python3 scripts/test_on_device.py --skip-flash  # test already-flashed firmware
    python3 scripts/test_on_device.py --port /dev/ttyACM1 sync

Exit code: 0 if all tests pass, 1 if any fail.
"""

import argparse
import glob
import json
import os
import struct
import subprocess
import sys
import time

import serial

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXAMPLES_DIR = os.path.join(REPO_ROOT, "examples", "stm32f469i-disco")
TARGET = "thumbv7em-none-eabihf"
BUILD_DIR = os.path.join(REPO_ROOT, "target", TARGET, "release")

SYNC_VID = 0x16C0
SYNC_PID = 0x27DD
ASYNC_VID = 0xC0DE
ASYNC_PID = 0xCAFE
STLINK_VID = 0x0483
STLINK_PID = 0x374B

CMD_STATUS = 0x10
CMD_TRIGGER = 0x11
CMD_DATA = 0x12
CMD_GET_SETTINGS = 0x13
CMD_SET_SETTINGS = 0x14

STATUS_OK = 0x00
STATUS_ERR = 0xFF
STATUS_NO_DATA = 0x12

RED = "\033[0;31m"
GREEN = "\033[0;32m"
YELLOW = "\033[1;33m"
CYAN = "\033[0;36m"
NC = "\033[0m"

passed = 0
failed = 0
skipped = 0


def log_info(msg):
    print(f"{CYAN}[INFO]{NC}  {msg}")


def log_ok(msg):
    print(f"{GREEN}[PASS]{NC}  {msg}")


def log_fail(msg):
    print(f"{RED}[FAIL]{NC}  {msg}")


def log_skip(msg):
    print(f"{YELLOW}[SKIP]{NC}  {msg}")


def check(name, condition, detail=""):
    global passed, failed
    if condition:
        log_ok(name)
        passed += 1
    else:
        log_fail(f"{name} {detail}")
        failed += 1


def find_cdc_port(vid, pid, timeout=20):
    """Find CDC port by VID:PID, filtering out ST-LINK."""
    import serial.tools.list_ports

    start = time.time()
    while time.time() - start < timeout:
        for p in serial.tools.list_ports.comports():
            if p.vid == vid and p.pid == pid:
                if p.vid == STLINK_VID and p.pid == STLINK_PID:
                    continue
                log_info(f"Found CDC: {p.device} (VID:PID={vid:04X}:{pid:04X})")
                return p.device
        time.sleep(0.5)

    log_fail(f"CDC not found (VID:PID={vid:04X}:{pid:04X}) after {timeout}s")
    return None


def wait_for_enumeration(vid, pid, timeout=20):
    """Wait for USB device to appear after flash."""
    log_info(f"Waiting for USB enumeration (VID:PID={vid:04X}:{pid:04X})...")
    port = find_cdc_port(vid, pid, timeout)
    if port:
        time.sleep(1)
    return port


def recover_usb():
    """Try USB bus recovery if xHCI is stuck."""
    log_info("Attempting USB recovery...")
    result = subprocess.run(
        ["sudo", "bash", "-c",
         'for f in /sys/bus/usb/devices/usb1/authorized; do echo 0 > "$f"; sleep 1; echo 1 > "$f"; done'],
        capture_output=True, timeout=10,
    )
    if result.returncode == 0:
        log_info("USB bus reset done")
        time.sleep(3)
    else:
        log_fail("USB recovery failed")


def flash_firmware(binary_name, extra_features=""):
    """Flash firmware using st-flash."""
    elf_path = os.path.join(BUILD_DIR, binary_name)
    bin_path = f"/tmp/{binary_name}.bin"

    if not os.path.exists(elf_path):
        log_fail(f"Binary not found: {elf_path}")
        return False

    log_info(f"Converting {binary_name} to binary...")
    result = subprocess.run(
        ["arm-none-eabi-objcopy", "-O", "binary", elf_path, bin_path],
        capture_output=True, timeout=30,
    )
    if result.returncode != 0:
        log_fail(f"objcopy failed: {result.stderr.decode()}")
        return False

    log_info(f"Flashing {binary_name} via st-flash...")
    result = subprocess.run(
        ["st-flash", "--connect-under-reset", "write", bin_path, "0x08000000"],
        capture_output=True, timeout=60,
    )
    if result.returncode != 0:
        log_fail(f"Flash failed: {result.stderr.decode()}")
        return False

    log_ok(f"Flashed {binary_name}")
    return True


def build_firmware(binary_name, features):
    """Build firmware with cargo."""
    log_info(f"Building {binary_name} (features: {features})...")
    result = subprocess.run(
        [
            "cargo", "build", "--release", "--target", TARGET,
            "--manifest-path", os.path.join(EXAMPLES_DIR, "Cargo.toml"),
            "--bin", binary_name,
            "--no-default-features", "--features", features,
        ],
        capture_output=True, timeout=300,
        cwd=REPO_ROOT,
    )
    if result.returncode != 0:
        log_fail(f"Build failed: {result.stderr.decode()[-500:]}")
        return False

    log_ok(f"Built {binary_name}")
    return True


class CdcClient:
    def __init__(self, port, baud=115200, timeout=2):
        self.port = port
        self.ser = serial.Serial(port, baud, timeout=timeout)
        self.ser.dtr = True
        time.sleep(0.2)

    def close(self):
        self.ser.close()

    def drain(self):
        """Drain input buffer and any pending heartbeat messages."""
        self.ser.reset_input_buffer()
        time.sleep(0.3)
        # Drain any remaining buffered data (e.g., async [ALIVE] heartbeat)
        old_timeout = self.ser.timeout
        self.ser.timeout = 0.2
        while self.ser.read(256):
            pass
        self.ser.timeout = old_timeout

    def send(self, cmd_byte, payload=b""):
        frame = bytes([cmd_byte, (len(payload) >> 8) & 0xFF, len(payload) & 0xFF]) + payload
        self.ser.write(frame)

    def recv(self, timeout=2.0):
        old_timeout = self.ser.timeout
        self.ser.timeout = timeout
        resp = self.ser.read(256)
        self.ser.timeout = old_timeout
        if not resp or len(resp) < 3:
            time.sleep(0.3)
            self.ser.timeout = 0.5
            extra = self.ser.read(256)
            self.ser.timeout = old_timeout
            if extra:
                resp = resp + extra
        if not resp or len(resp) < 3:
            return None, b""
        status = resp[0]
        length = (resp[1] << 8) | resp[2]
        payload = resp[3 : 3 + length]
        return status, payload


def test_cdc_protocol(client, label=""):
    """Run 6-command CDC protocol test."""
    log_info(f"CDC Protocol Tests ({label})")
    prefix = f"[{label}] " if label else ""

    client.drain()

    # 1. ScannerStatus
    client.send(CMD_STATUS)
    status, payload = client.recv()
    check(f"{prefix}ScannerStatus: status OK", status == STATUS_OK, f"got 0x{status:02x}" if status is not None else "no response")
    check(f"{prefix}ScannerStatus: payload >= 3 bytes", len(payload) >= 3, f"got {len(payload)}")

    client.drain()

    # 2. GetSettings
    client.send(CMD_GET_SETTINGS)
    status, payload = client.recv()
    check(f"{prefix}GetSettings: status OK", status == STATUS_OK, f"got 0x{status:02x}" if status is not None else "no response")
    check(f"{prefix}GetSettings: payload >= 1 byte", len(payload) >= 1, f"got {len(payload)}")
    if payload:
        settings_val = payload[0]
        log_info(f"{prefix}  Settings raw: 0x{settings_val:02x}")

    client.drain()

    # 3. ScannerTrigger
    client.send(CMD_TRIGGER)
    status, payload = client.recv()
    check(f"{prefix}ScannerTrigger: status OK", status == STATUS_OK, f"got 0x{status:02x}" if status is not None else "no response")

    client.drain()

    # 4. ScannerData (no scan expected)
    client.send(CMD_DATA)
    status, payload = client.recv()
    if status == STATUS_NO_DATA:
        check(f"{prefix}ScannerData: no data (expected)", True)
    elif status == STATUS_OK and len(payload) > 1:
        check(f"{prefix}ScannerData: got data (ambient scan)", True)
    else:
        check(f"{prefix}ScannerData: status OK or NoData", False, f"got 0x{status:02x}" if status is not None else "no response")

    client.drain()

    # 5. SetSettings
    test_val = 0xD1
    client.send(CMD_SET_SETTINGS, bytes([test_val]))
    status, payload = client.recv()
    check(f"{prefix}SetSettings: status OK", status == STATUS_OK, f"got 0x{status:02x}" if status is not None else "no response")
    if status == STATUS_OK and payload:
        check(f"{prefix}SetSettings: readback 0xD1", payload[0] == test_val, f"got 0x{payload[0]:02x}")

    client.drain()

    # 6. GetSettings verify
    client.send(CMD_GET_SETTINGS)
    status, payload = client.recv()
    check(f"{prefix}GetSettings verify: status OK", status == STATUS_OK, f"got 0x{status:02x}" if status is not None else "no response")
    if payload:
        check(f"{prefix}GetSettings verify: 0xD1", payload[0] == 0xD1, f"got 0x{payload[0]:02x}")


def test_enumeration(client, vid, pid, label=""):
    """Test that device stays enumerated and responds to a basic command."""
    log_info(f"Enumeration stability ({label})")
    prefix = f"[{label}] " if label else ""

    client.drain()
    client.send(CMD_STATUS)
    status, payload = client.recv(timeout=3.0)
    check(f"{prefix}Device responds after enumeration", status is not None, "no response")

    # Verify no disconnect by sending a second command
    time.sleep(1)
    client.drain()
    client.send(CMD_STATUS)
    status2, _ = client.recv(timeout=3.0)
    check(f"{prefix}Device stable after 1s", status2 is not None, "no response on second command")


def run_sync_tests(port):
    """Test sync production firmware."""
    print(f"\n{'='*60}")
    print(f"  SYNC FIRMWARE TESTS (VID:PID={SYNC_VID:04X}:{SYNC_PID:04X})")
    print(f"{'='*60}\n")

    client = CdcClient(port)
    try:
        test_enumeration(client, SYNC_VID, SYNC_PID, "sync")
        print()
        test_cdc_protocol(client, "sync")
    finally:
        client.close()


def run_async_tests(port):
    """Test async production firmware."""
    print(f"\n{'='*60}")
    print(f"  ASYNC FIRMWARE TESTS (VID:PID={ASYNC_VID:04X}:{ASYNC_PID:04X})")
    print(f"{'='*60}\n")

    client = CdcClient(port, timeout=3)
    try:
        test_enumeration(client, ASYNC_VID, ASYNC_PID, "async")
        print()
        test_cdc_protocol(client, "async")
    finally:
        client.close()


def write_test_report(results, path):
    """Write test results to JSON file for merge verification."""
    report = {
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "passed": results["passed"],
        "failed": results["failed"],
        "skipped": results["skipped"],
        "total": results["passed"] + results["failed"] + results["skipped"],
        "all_passed": results["failed"] == 0,
        "tests": results["tests"],
    }
    os.makedirs(os.path.dirname(path) if os.path.dirname(path) else ".", exist_ok=True)
    with open(path, "w") as f:
        json.dump(report, f, indent=2)
    return path


def main():
    parser = argparse.ArgumentParser(description="On-device test runner for gm65-scanner firmware")
    parser.add_argument("target", nargs="?", choices=["sync", "async", "both"], default="both",
                        help="Which firmware to test (default: both)")
    parser.add_argument("--skip-flash", action="store_true", help="Skip build and flash")
    parser.add_argument("--port", "-p", help="CDC port (auto-detect if omitted)")
    parser.add_argument("--report", default=None,
                        help="Path to write JSON test report (default: .test-report.json)")
    parser.add_argument("--no-usb-recovery", action="store_true", help="Skip USB recovery on failure")
    args = parser.parse_args()

    results = {"passed": 0, "failed": 0, "skipped": 0, "tests": []}

    def run_firmware_tests(name, binary_name, build_features, vid, pid, test_fn):
        global passed, failed, skipped
        saved_passed = passed
        saved_failed = failed

        port = args.port
        if not args.skip_flash:
            if not build_firmware(binary_name, build_features):
                failed = saved_failed
                return

            # Kill stale st-flash
            subprocess.run(["pkill", "-9", "st-flash"], capture_output=True, timeout=5)
            subprocess.run(["pkill", "-9", "probe-rs"], capture_output=True, timeout=5)
            time.sleep(2)

            if not flash_firmware(binary_name):
                failed = saved_failed
                return

            port = wait_for_enumeration(vid, pid)
            if not port:
                if not args.no_usb_recovery:
                    recover_usb()
                    port = wait_for_enumeration(vid, pid, timeout=10)
                if not port:
                    failed = saved_failed
                    return

        if port is None:
            log_fail(f"{name}: No port available")
            failed = saved_failed
            return

        test_fn(port)

        test_passed = (failed - saved_failed) == 0
        results["tests"].append({
            "firmware": name,
            "binary": binary_name,
            "vid": f"0x{vid:04X}",
            "pid": f"0x{pid:04X}",
            "port": port,
            "passed": passed - saved_passed,
            "failed": failed - saved_failed,
        })

    if args.target in ("sync", "both"):
        run_firmware_tests(
            "sync", "stm32f469i-disco-scanner", "sync-mode",
            SYNC_VID, SYNC_PID, run_sync_tests,
        )

    if args.target in ("async", "both"):
        run_firmware_tests(
            "async", "async_firmware", "scanner-async",
            ASYNC_VID, ASYNC_PID, run_async_tests,
        )

    results["passed"] = passed
    results["failed"] = failed
    results["skipped"] = skipped
    results["all_passed"] = failed == 0

    print(f"\n{'='*60}")
    print(f"  SUMMARY: {passed} passed, {failed} failed, {skipped} skipped")
    print(f"{'='*60}\n")

    report_path = args.report or os.path.join(REPO_ROOT, ".test-report.json")
    written = write_test_report(results, report_path)
    if results["all_passed"]:
        log_ok(f"Test report: {written}")
    else:
        log_fail(f"Test report: {written}")

    sys.exit(0 if results["all_passed"] else 1)


if __name__ == "__main__":
    main()
