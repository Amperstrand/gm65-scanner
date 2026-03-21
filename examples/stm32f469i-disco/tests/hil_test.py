#!/usr/bin/env python3
"""
Hardware-in-the-Loop test harness for gm65-scanner STM32F469I firmware.

Usage:
    python3 hil_test.py --port /dev/ttyACM1 protocol
    python3 hil_test.py --port /dev/ttyACM1 e2e
    python3 hil_test.py --port /dev/ttyACM1 scan  (wait for manual scan)

CDC Protocol:
    Commands: [cmd_byte, len_hi, len_lo, payload...]
    Responses: [status, len_hi, len_lo, payload...]

    0x10 = ScannerStatus
    0x11 = ScannerTrigger
    0x12 = ScannerData
    0x13 = GetSettings
    0x14 = SetSettings
"""

import argparse
import glob
import sys
import time

import serial

CMD_STATUS = 0x10
CMD_TRIGGER = 0x11
CMD_DATA = 0x12
CMD_GET_SETTINGS = 0x13
CMD_SET_SETTINGS = 0x14

STATUS_OK = 0x00
STATUS_ERR = 0xFF
STATUS_NO_DATA = 0x12

TYPE_NAMES = {
    0x00: "PlainText/URL",
    0x01: "CashuV4",
    0x02: "CashuV3",
    0x03: "URFragment",
    0x04: "Binary",
}

MODEL_NAMES = {
    0x00: "Unknown",
    0x01: "GM65",
    0x02: "M3Y",
    0x03: "Generic",
}


class CdcClient:
    def __init__(self, port, baud=115200, timeout=2):
        self.ser = serial.Serial(port, baud, timeout=timeout)
        self.ser.dtr = True
        time.sleep(0.2)

    def close(self):
        self.ser.close()

    def send(self, cmd_byte, payload=b""):
        frame = bytes([cmd_byte, (len(payload) >> 8) & 0xFF, len(payload) & 0xFF]) + payload
        self.ser.write(frame)

    def recv(self, timeout=1.0):
        old_timeout = self.ser.timeout
        self.ser.timeout = timeout
        resp = self.ser.read(256)
        self.ser.timeout = old_timeout
        if not resp or len(resp) < 3:
            return None, b""
        status = resp[0]
        length = (resp[1] << 8) | resp[2]
        payload = resp[3 : 3 + length]
        return status, payload

    def drain(self):
        self.ser.reset_input_buffer()

    def find_port(cls, vid=0x16C0, pid=0x27DD):
        import serial.tools.list_ports

        for p in serial.tools.list_ports.comports():
            if p.vid == vid and p.pid == pid:
                return p.device
        return None


def find_cdc_port():
    import serial.tools.list_ports

    for p in serial.tools.list_ports.comports():
        if p.vid == 0x16C0 and p.pid == 0x27DD:
            return p.device

    ports = sorted(glob.glob("/dev/ttyACM*"))
    for port in ports:
        try:
            ser = serial.Serial(port, 115200, timeout=1)
            ser.dtr = True
            time.sleep(0.1)
            ser.write(bytes([CMD_STATUS, 0x00, 0x00]))
            time.sleep(0.3)
            resp = ser.read(64)
            ser.close()
            if resp and len(resp) >= 3 and resp[0] == STATUS_OK:
                return port
        except Exception:
            pass
    return None


def open_cdc(port=None, retries=10):
    if port is None:
        port = find_cdc_port()

    if port is None:
        print("ERROR: No CDC port found. Is the firmware running?")
        sys.exit(1)

    print(f"Using port: {port}")

    for attempt in range(retries):
        try:
            client = CdcClient(port)
            client.send(CMD_STATUS)
            status, payload = client.recv(timeout=1.0)
            if status == STATUS_OK and len(payload) >= 3:
                print(f"CDC connected (attempt {attempt + 1})")
                return client, port
            client.close()
        except Exception as e:
            print(f"Attempt {attempt + 1}: {e}")
        time.sleep(0.5)

    print("ERROR: Could not establish CDC communication")
    sys.exit(1)


def test_protocol(client):
    passed = 0
    failed = 0

    def check(name, condition, detail=""):
        nonlocal passed, failed
        if condition:
            print(f"  PASS: {name}")
            passed += 1
        else:
            print(f"  FAIL: {name} {detail}")
            failed += 1

    print("\n=== CDC Protocol Tests ===\n")

    # Test 1: ScannerStatus
    print("1. ScannerStatus")
    client.send(CMD_STATUS)
    status, payload = client.recv()
    check("status == OK", status == STATUS_OK, f"got 0x{status:02x}")
    check("payload >= 3 bytes", len(payload) >= 3, f"got {len(payload)}")
    if len(payload) >= 3:
        connected = payload[0]
        initialized = payload[1]
        model = payload[2]
        check("scanner connected", connected == 1, f"got {connected}")
        check("scanner initialized", initialized == 1, f"got {initialized}")
        model_name = MODEL_NAMES.get(model, f"0x{model:02x}")
        print(f"    Model: {model_name} (0x{model:02x})")

    # Test 2: GetSettings
    print("\n2. GetSettings")
    client.send(CMD_GET_SETTINGS)
    status, payload = client.recv()
    check("status == OK", status == STATUS_OK, f"got 0x{status:02x}")
    check("payload >= 1 byte", len(payload) >= 1, f"got {len(payload)}")
    if payload:
        raw = payload[0]
        always_on = bool(raw & 0x80)
        sound = bool(raw & 0x40)
        aim = bool(raw & 0x10)
        light = bool(raw & 0x04)
        continuous = bool(raw & 0x02)
        command = bool(raw & 0x01)
        print(f"    Raw: 0x{raw:02x}")
        print(f"    ALWAYS_ON={always_on} SOUND={sound} AIM={aim} LIGHT={light} CONT={continuous} CMD={command}")
        check("ALWAYS_ON set", always_on)
        check("SOUND set", sound)
        check("COMMAND mode", command, "scanner should be in command-triggered mode")
        check("not CONTINUOUS", not continuous, "continuous mode is broken on our scanner")

    # Test 3: ScannerTrigger
    print("\n3. ScannerTrigger")
    client.send(CMD_TRIGGER)
    status, payload = client.recv()
    check("trigger status == OK", status == STATUS_OK, f"got 0x{status:02x}")

    # Test 4: ScannerData
    print("\n4. ScannerData")
    time.sleep(1)
    client.send(CMD_DATA)
    status, payload = client.recv()
    if status == STATUS_OK and len(payload) > 1:
        type_byte = payload[0]
        data = payload[1:]
        type_name = TYPE_NAMES.get(type_byte, f"Unknown(0x{type_byte:02x})")
        try:
            text = data.decode("utf-8", errors="replace")[:80]
        except Exception:
            text = repr(data[:80])
        print(f"  Got scan data! Type: {type_name}, Length: {len(data)}")
        print(f"  Data: {text}")
        check("scan data received (scanner is working)", True)
    elif status == STATUS_NO_DATA:
        check("no scan data (as expected, no QR in view)", True)
    else:
        check("status == OK or NoScanData", False, f"got 0x{status:02x}")

    # Test 5: SetSettings
    print("\n5. SetSettings")
    test_val = 0xD1  # ALWAYS_ON | SOUND | AIM | COMMAND
    client.send(CMD_SET_SETTINGS, bytes([test_val]))
    status, payload = client.recv()
    check("set status == OK", status == STATUS_OK, f"got 0x{status:02x}")
    if status == STATUS_OK and payload:
        check("readback matches", payload[0] == test_val, f"got 0x{payload[0]:02x}")

    # Test 6: Verify settings persisted
    print("\n6. GetSettings (after set)")
    client.send(CMD_GET_SETTINGS)
    status, payload = client.recv()
    check("status == OK", status == STATUS_OK, f"got 0x{status:02x}")
    if payload:
        check("settings == 0xD1", payload[0] == 0xD1, f"got 0x{payload[0]:02x}")

    print(f"\n=== Results: {passed} passed, {failed} failed ===\n")
    return failed == 0


def test_e2e_scan(client, qr_text="https://example.com/gm65-test"):
    import qrcode
    from PIL import Image

    print("\n=== E2E Scan Test ===\n")

    qr_path = "/tmp/qr.png"
    print(f"Generating QR code: '{qr_text}'")
    img = qrcode.make(qr_text, box_size=10, border=4)
    img.save(qr_path)
    print(f"QR code saved to {qr_path}")
    print(f"QR size: {img.size[0]}x{img.size[1]} pixels")

    img.show()
    print("\n>>> QR code displayed on screen <<<")
    print(">>> Hold the QR code in front of the scanner NOW <<<")

    poll_start = time.time()
    max_wait = 30
    found = False

    while time.time() - poll_start < max_wait:
        client.drain()
        client.send(CMD_DATA)
        status, payload = client.recv(timeout=0.5)
        if status == STATUS_OK and len(payload) > 1:
            type_byte = payload[0]
            data = payload[1:]
            try:
                text = data.decode("utf-8", errors="replace")
            except Exception:
                text = repr(data)
            type_name = TYPE_NAMES.get(type_byte, f"Unknown(0x{type_byte:02x})")
            print(f"\nSCAN DETECTED after {time.time() - poll_start:.1f}s")
            print(f"  Type: {type_name}")
            print(f"  Length: {len(data)} bytes")
            print(f"  Data: {text[:200]}")
            if qr_text in text:
                print("  MATCH: scanned data contains expected text")
            else:
                print(f"  WARNING: expected '{qr_text}' in scan data")
            found = True
            break
        elif status is not None:
            print(f"  ...waiting (status=0x{status:02x}) [{time.time() - poll_start:.0f}s]")
        time.sleep(0.3)

    if not found:
        print(f"\nTIMEOUT: No scan data received after {max_wait}s")
        return False

    return True


def wait_for_manual_scan(client, timeout=60):
    print("\n=== Waiting for Manual Scan ===\n")
    print(">>> Scan any QR code with the scanner <<<")
    print(f"    (timeout: {timeout}s)")

    poll_start = time.time()
    found = False

    while time.time() - poll_start < timeout:
        client.drain()
        client.send(CMD_DATA)
        status, payload = client.recv(timeout=0.5)
        if status == STATUS_OK and len(payload) > 1:
            type_byte = payload[0]
            data = payload[1:]
            try:
                text = data.decode("utf-8", errors="replace")
            except Exception:
                text = repr(data)
            type_name = TYPE_NAMES.get(type_byte, f"Unknown(0x{type_byte:02x})")
            elapsed = time.time() - poll_start
            print(f"\nSCAN DETECTED after {elapsed:.1f}s")
            print(f"  Type: {type_name}")
            print(f"  Length: {len(data)} bytes")
            print(f"  Data: {text[:200]}")
            found = True
            break
        elif status is not None:
            elapsed = time.time() - poll_start
            if int(elapsed) % 5 == 0:
                print(f"  ...waiting [{int(elapsed)}s]", end="\r")
        time.sleep(0.3)

    if not found:
        print(f"\nTIMEOUT: No scan data after {timeout}s")
        return False

    return True


def main():
    parser = argparse.ArgumentParser(description="HIL test harness for gm65-scanner firmware")
    parser.add_argument("--port", "-p", help="CDC port (auto-detect if omitted)")
    parser.add_argument(
        "command",
        choices=["protocol", "e2e", "scan"],
        help="Test to run: protocol (CDC tests), e2e (scan QR from screen), scan (wait for manual scan)",
    )
    parser.add_argument("--qr-text", default="https://example.com/gm65-test", help="QR code text for E2E test")
    parser.add_argument("--timeout", type=int, default=30, help="Scan wait timeout in seconds")
    args = parser.parse_args()

    client, port = open_cdc(args.port)
    try:
        if args.command == "protocol":
            ok = test_protocol(client)
            sys.exit(0 if ok else 1)
        elif args.command == "e2e":
            ok = test_e2e_scan(client, args.qr_text)
            sys.exit(0 if ok else 1)
        elif args.command == "scan":
            ok = wait_for_manual_scan(client, args.timeout)
            sys.exit(0 if ok else 1)
    finally:
        client.close()


if __name__ == "__main__":
    main()
