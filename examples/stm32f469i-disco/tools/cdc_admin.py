#!/usr/bin/env python3
"""Minimal POSIX CDC admin helper for the async DS2208-compatible firmware."""

from __future__ import annotations

import argparse
import os
import sys
import termios
import time


def configure_tty(fd: int, baud: int) -> None:
    attrs = termios.tcgetattr(fd)
    attrs[0] = 0
    attrs[1] = 0
    attrs[2] = attrs[2] | termios.CREAD | termios.CLOCAL | termios.CS8
    attrs[3] = 0
    attrs[6][termios.VMIN] = 0
    attrs[6][termios.VTIME] = 1
    baud_const = getattr(termios, f"B{baud}")
    termios.cfsetispeed(attrs, baud_const)
    termios.cfsetospeed(attrs, baud_const)
    termios.tcsetattr(fd, termios.TCSANOW, attrs)


def read_available(fd: int, timeout: float) -> bytes:
    deadline = time.monotonic() + timeout
    chunks: list[bytes] = []
    while time.monotonic() < deadline:
        chunk = os.read(fd, 512)
        if chunk:
            chunks.append(chunk)
            deadline = time.monotonic() + 0.2
        else:
            time.sleep(0.02)
    return b"".join(chunks)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("tty", help="CDC ACM device, e.g. /dev/ttyACM0")
    parser.add_argument("frame_hex", help="Frame bytes as hex, e.g. 5a 23 00")
    parser.add_argument("--baud", type=int, default=115200)
    parser.add_argument("--timeout", type=float, default=1.0)
    args = parser.parse_args()

    frame = bytes.fromhex(args.frame_hex)
    fd = os.open(args.tty, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    try:
        configure_tty(fd, args.baud)
        os.write(fd, frame)
        response = read_available(fd, args.timeout)
    finally:
        os.close(fd)

    sys.stdout.buffer.write(response)
    if response and not response.endswith(b"\n"):
        sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
