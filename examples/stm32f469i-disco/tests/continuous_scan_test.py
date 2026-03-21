#!/usr/bin/env python3
"""
Continuous E2E scanner test for gm65-scanner firmware.

Leave the scanner pointed at a QR code. This script polls ScannerData
via CDC and reports every scan result. Use during firmware development
to verify scan data flows through after each flash.

Usage:
    python3 continuous_scan_test.py [--port /dev/ttyACM1] [--timeout 120]

Press Ctrl+C to stop and get a summary.
"""

import signal
import sys
import time

import serial

CMD_STATUS = 0x10
CMD_DATA = 0x12
STATUS_OK = 0x00
STATUS_NO_DATA = 0x12

TYPE_NAMES = {
    0x00: "Text/URL",
    0x01: "CashuV4",
    0x02: "CashuV3",
    0x03: "UR",
    0x04: "Binary",
}


class Stats:
    def __init__(self):
        self.scans = 0
        self.errors = 0
        self.timeouts = 0
        self.last_data = None
        self.start_time = time.time()

    def elapsed(self):
        return time.time() - self.start_time

    def summary(self):
        rate = self.scans / self.elapsed() * 60 if self.elapsed() > 0 else 0
        lines = [
            "",
            "=" * 50,
            f"  Scans received:  {self.scans}",
            f"  Errors:          {self.errors}",
            f"  Timeouts:        {self.timeouts}",
            f"  Duration:        {self.elapsed():.0f}s",
            f"  Scan rate:       {rate:.1f}/min",
            "=" * 50,
        ]
        if self.last_data:
            lines.append(f"  Last scan ({len(self.last_data)} bytes):")
            try:
                text = self.last_data.decode("utf-8", errors="replace")
                for i in range(0, len(text), 70):
                    lines.append(f"    {text[i:i+70]}")
            except Exception:
                lines.append(f"    {self.last_data[:70].hex()}")
        return "\n".join(lines)


def find_port():
    import glob
    import serial.tools.list_ports

    for p in serial.tools.list_ports.comports():
        if p.vid == 0x16C0 and p.pid == 0x27DD:
            return p.device
    for port in sorted(glob.glob("/dev/ttyACM*")):
        try:
            ser = serial.Serial(port, 115200, timeout=1)
            ser.dtr = True
            time.sleep(0.1)
            ser.write(bytes([CMD_STATUS, 0x00, 0x00]))
            time.sleep(0.3)
            resp = ser.read(64)
            ser.close()
            if resp and len(resp) >= 3 and resp[0] == STATUS_OK and len(resp) >= 6:
                if resp[3] == 1:  # connected
                    return port
        except Exception:
            pass
    return None


def main():
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument("--port", "-p", default=None)
    parser.add_argument("--timeout", "-t", type=int, default=120, help="Total test duration in seconds")
    args = parser.parse_args()

    port = args.port or find_port()
    if not port:
        print("ERROR: No CDC port found. Is firmware running?")
        sys.exit(1)

    stats = Stats()

    def on_signal(sig, frame):
        print(stats.summary())
        sys.exit(0)

    signal.signal(signal.SIGINT, on_signal)

    print(f"Continuous scan test on {port} (timeout={args.timeout}s)")
    print("Scanner should be pointed at a QR code.")
    print("Press Ctrl+C for summary.\n")

    deadline = time.time() + args.timeout

    while time.time() < deadline:
        try:
            ser = serial.Serial(port, 115200, timeout=1)
        except Exception as e:
            print(f"  [ERROR] Cannot open {port}: {e}")
            stats.errors += 1
            time.sleep(2)
            continue

        try:
            ser.dtr = True
            time.sleep(0.05)

            ser.write(bytes([CMD_DATA, 0x00, 0x00]))
            time.sleep(0.3)
            resp = ser.read(256)

            if resp and len(resp) >= 3:
                status = resp[0]
                length = (resp[1] << 8) | resp[2]
                payload = resp[3 : 3 + length]

                if status == STATUS_OK and len(payload) > 1:
                    type_byte = payload[0]
                    data = payload[1:]
                    type_name = TYPE_NAMES.get(type_byte, f"?0x{type_byte:02x}")
                    stats.scans += 1
                    stats.last_data = data
                    try:
                        text = data.decode("utf-8", errors="replace")[:60]
                    except Exception:
                        text = data[:30].hex()
                    print(
                        f"  [{stats.elapsed():6.1f}s] SCAN #{stats.scans}: "
                        f"{type_name} {len(data)}B \"{text}\""
                    )
                elif status == STATUS_NO_DATA:
                    stats.timeouts += 1
                else:
                    stats.errors += 1
                    print(f"  [{stats.elapsed():6.1f}s] ERROR status=0x{status:02x}")
            else:
                stats.timeouts += 1

        except Exception as e:
            stats.errors += 1
            print(f"  [{stats.elapsed():6.1f}s] EXCEPTION: {e}")
        finally:
            ser.close()

        time.sleep(0.5)

    print(stats.summary())


if __name__ == "__main__":
    main()
