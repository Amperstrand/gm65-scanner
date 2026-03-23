#!/usr/bin/env python3
"""CDC quick-test runner for gm65-scanner firmware.

Tests all CDC binary protocol commands and prints pass/fail results.
Returns exit code 0 (all pass) or 1 (any fail).

Usage:
    python3 scripts/cdc-test.py --port /dev/ttyACM1
    python3 scripts/cdc-test.py --port /dev/ttyACM1 --verbose
    python3 scripts/cdc-test.py --port /dev/ttyACM1 --qr "Hello World"
"""

import argparse
import serial
import sys
import time


def open_port(port, retries=3):
    for attempt in range(retries):
        try:
            ser = serial.Serial(port, 115200, timeout=2)
            ser.dtr = True
            time.sleep(0.3)
            ser.read(256)
            return ser
        except (serial.SerialException, FileNotFoundError) as e:
            if attempt < retries - 1:
                time.sleep(2)
            else:
                return None


def send_cmd(ser, cmd_byte, payload=b''):
    frame = bytes([cmd_byte, (len(payload) >> 8) & 0xFF, len(payload) & 0xFF]) + payload
    ser.write(frame)
    for _ in range(10):
        time.sleep(0.3)
        resp = ser.read(256)
        if resp:
            return resp
    return b''


def test_scanner_status(ser, verbose=False):
    resp = send_cmd(ser, 0x10)
    if not resp:
        return False, "no response"
    if resp[0] != 0x00:
        return False, f"status=0x{resp[0]:02x}"
    if len(resp) < 4:
        return False, f"payload too short ({len(resp)} bytes)"
    connected = resp[3] & 0x01
    initialized = resp[4] & 0x01 if len(resp) > 4 else 0
    if verbose:
        print(f"    connected={connected} initialized={initialized}")
    return True, f"connected={connected} initialized={initialized}"


def test_get_settings(ser, verbose=False):
    resp = send_cmd(ser, 0x13)
    if not resp:
        return False, "no response"
    if resp[0] != 0x00:
        return False, f"status=0x{resp[0]:02x}"
    if len(resp) < 4:
        return False, "no payload"
    settings = resp[3]
    if verbose:
        print(f"    settings=0x{settings:02x}")
    return True, f"settings=0x{settings:02x}"


def test_display_qr(ser, qr_text, verbose=False):
    resp = send_cmd(ser, 0x15, qr_text.encode('utf-8'))
    if not resp:
        return False, "no response"
    if resp[0] != 0x00:
        return False, f"status=0x{resp[0]:02x}"
    return True, f"QR rendered: {qr_text!r}"


def test_scanner_trigger(ser, verbose=False):
    resp = send_cmd(ser, 0x11)
    if not resp:
        return False, "no response"
    if resp[0] != 0x00:
        return False, f"status=0x{resp[0]:02x}"
    return True, "triggered"


def test_scanner_data(ser, verbose=False):
    resp = send_cmd(ser, 0x12)
    if not resp:
        return False, "no response"
    if resp[0] == 0x12:
        return True, "NoScanData (expected if no scan yet)"
    if resp[0] != 0x00:
        return False, f"status=0x{resp[0]:02x}"
    if len(resp) > 4:
        type_byte = resp[3]
        type_names = {0x00: "Plain/URL", 0x01: "CashuV4", 0x02: "CashuV3",
                      0x03: "UR Fragment", 0x04: "Binary"}
        type_name = type_names.get(type_byte, f"Unknown(0x{type_byte:02x})")
        data_len = len(resp) - 4
        if verbose:
            print(f"    type={type_name} data_len={data_len}")
        return True, f"type={type_name} data_len={data_len}"
    return True, "OK (no payload)"


def test_enter_settings(ser, verbose=False):
    resp = send_cmd(ser, 0x16)
    if not resp:
        return False, "no response"
    if resp[0] != 0x00:
        return False, f"status=0x{resp[0]:02x}"
    return True, "settings UI shown"


def run_tests(port, qr_text="test", verbose=False):
    print(f"CDC Test — {port}")
    print("=" * 60)

    ser = open_port(port)
    if ser is None:
        print(f"[FAIL] Cannot open {port}")
        return False

    tests = [
        ("SCANNER_STATUS  (0x10)", lambda s: test_scanner_status(s, verbose)),
        ("GET_SETTINGS    (0x13)", lambda s: test_get_settings(s, verbose)),
        ("DISPLAY_QR      (0x15)", lambda s: test_display_qr(s, qr_text, verbose)),
        ("SCANNER_TRIGGER (0x11)", lambda s: test_scanner_trigger(s, verbose)),
        ("SCANNER_DATA    (0x12)", lambda s: test_scanner_data(s, verbose)),
        ("ENTER_SETTINGS  (0x16)", lambda s: test_enter_settings(s, verbose)),
    ]

    passed = 0
    failed = 0
    for name, test_fn in tests:
        try:
            ok, detail = test_fn(ser)
            status = "PASS" if ok else "FAIL"
            if ok:
                passed += 1
                print(f"  [PASS] {name} — {detail}")
            else:
                failed += 1
                print(f"  [FAIL] {name} — {detail}")
        except Exception as e:
            failed += 1
            print(f"  [FAIL] {name} — exception: {e}")

    ser.close()
    print("=" * 60)
    print(f"Results: {passed} passed, {failed} failed, {passed + failed} total")
    return failed == 0


def main():
    parser = argparse.ArgumentParser(description="CDC quick-test for gm65-scanner firmware")
    parser.add_argument("--port", required=True, help="Serial port (e.g. /dev/ttyACM1)")
    parser.add_argument("--qr", default="test", help="Text for DISPLAY_QR test")
    parser.add_argument("--verbose", "-v", action="store_true", help="Show extra detail")
    args = parser.parse_args()

    ok = run_tests(args.port, qr_text=args.qr, verbose=args.verbose)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
