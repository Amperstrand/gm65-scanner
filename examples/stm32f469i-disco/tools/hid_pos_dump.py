#!/usr/bin/env python3
"""Read fixed-size HID POS reports from a Linux hidraw node."""

from __future__ import annotations

import argparse
import os
import struct


REPORT_LEN = 261


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("hidraw", help="hidraw node, e.g. /dev/hidraw3")
    args = parser.parse_args()

    with open(args.hidraw, "rb", buffering=0) as fh:
        while True:
            report = fh.read(REPORT_LEN)
            if len(report) != REPORT_LEN:
                continue
            payload_len = struct.unpack_from("<H", report, 256)[0]
            payload = report[:payload_len]
            symbology = report[258:261]
            print(
                {
                    "payload_len": payload_len,
                    "payload_hex": payload.hex(),
                    "payload_text": payload.decode("utf-8", errors="replace"),
                    "symbology": symbology.hex(),
                }
            )


if __name__ == "__main__":
    raise SystemExit(main())
