"""QR loopback rig: CYD QR source + GM65/F469 scanner, host plumbing.

Topology (bench, 2026-09-08):
    CYD (ESP32-2432S028, ILI9341) --CH340 ttyUSB--> host serial line protocol
    CYD screen  <--points at-->  GM65 module --USART6--> STM32F469I-DISCO
    F469 user USB --CDC c0de:cafe--> host (gm65 async firmware)

Safety: fips-lab boards.toml is the flash gate (one registry per bench —
microfips devices.py precedent). The F469 board is shared with the micronuts
wallet: sessions MUST backup the 2MB flash before overwriting and restore it
afterwards (backup_stm32 / restore_stm32).
"""

from __future__ import annotations

import json
import os
import subprocess
import time
import tomllib
from dataclasses import dataclass
from pathlib import Path

import serial

REPO_ROOT = Path(__file__).resolve().parents[2]
BOARDS_TOML = REPO_ROOT.parent / "fips-lab" / "fips_lab" / "boards.toml"

CYD_REGISTRY_KEY = "cyd-ch340"
CYD_ID_PATH = "pci-0000:02:00.0-usb-0:1:1.0-port0"
CYD_VIDPID = (0x1A86, 0x7523)

STM32_REGISTRY_KEY = "stm32f469i-disco"
STLINK_SERIAL = "066FFF515786534867184152"
STLINK_VIDPID = (0x0483, 0x374B)

GM65_CDC_VIDPID = (0xC0DE, 0xCAFE)  # gm65-scanner async firmware
WALLET_CDC_VIDPID = (0x16C0, 0x27DD)  # micronuts wallet firmware
WALLET_CDC_SERIAL = "F4691"

STM32_FLASH_SIZE = 0x200000  # 2 MiB (STM32F469NI)
STM32_FLASH_BASE = 0x08000000

CMD_STATUS = 0x10
CMD_TRIGGER = 0x11
CMD_DATA = 0x12
STATUS_OK = 0x00
STATUS_NO_DATA = 0x12


class RigError(RuntimeError):
    pass


def require_board(key: str, op: str) -> None:
    path = Path(os.environ.get("GM65_BOARDS_TOML", BOARDS_TOML))
    with open(path, "rb") as f:
        data = tomllib.load(f)
    spec = data.get("boards", {}).get(key)
    if spec is None:
        raise RigError(f"board {key!r} is not in {path} — refusing {op}")
    if op not in spec.get("ops", []):
        raise RigError(f"board {key!r} does not permit {op!r} (allowed: {spec.get('ops')})")


def cyd_port() -> Path:
    by_path = Path("/dev/serial/by-path") / CYD_ID_PATH
    if not by_path.exists():
        raise RigError(
            f"CYD CH340 not attached (expected /dev/serial/by-path/{CYD_ID_PATH})"
        )
    return by_path.resolve()


def find_serial_port(vidpid, serial_number=None, product=None, timeout=15.0):
    import serial.tools.list_ports

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        for p in serial.tools.list_ports.comports():
            if (p.vid, p.pid) != vidpid:
                continue
            if serial_number is not None and p.serial_number != serial_number:
                continue
            if product is not None and (p.product or "") != product:
                continue
            return p.device
        time.sleep(0.5)
    return None


def find_serial_by_id(id_substring, timeout=15.0):
    """Match /dev/serial/by-id entries on a substring (pyserial's .product is
    udev-dependent and often None for these CDCs)."""
    import glob as _glob

    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        matches = _glob.glob(f"/dev/serial/by-id/*{id_substring}*")
        if matches:
            return matches[0]
        time.sleep(0.5)
    return None


def recover_xhci_port():
    """The F469 user-USB port (bus 3 = PCI 0000:07:00.3) wedges host-side
    after flash cycles — the device never re-enumerates until the controller
    is bounced. Bench-verified 2026-09-10 (kernel shows clean disconnect, no
    reconnect; PCI remove+rescan restores it instantly). Safe: nothing else
    lives on that controller."""
    import subprocess as sp

    for cmd in (
        ["sudo", "tee", "/sys/bus/pci/devices/0000:07:00.3/remove"],
        ["sudo", "tee", "/sys/bus/pci/rescan"],
    ):
        proc = sp.run(cmd, input=b"1\n", capture_output=True)
        if proc.returncode != 0:
            raise RigError(f"xhci recovery failed at {' '.join(cmd[-2:])}")
        time.sleep(2)


def wait_serial_by_id_healed(id_substring, timeout=30.0):
    """wait_serial_by_id with a one-shot xHCI bounce if the CDC stays absent
    past the first half of the window."""
    import glob as _glob

    deadline = time.monotonic() + timeout
    bounced = False
    while time.monotonic() < deadline:
        matches = _glob.glob(f"/dev/serial/by-id/*{id_substring}*")
        if matches:
            return matches[0]
        if not bounced and time.monotonic() > deadline - timeout / 2:
            recover_xhci_port()
            bounced = True
        time.sleep(0.5)
    return None


def wait_serial_port(vidpid, serial_number=None, timeout=15.0):
    deadline = time.monotonic() + timeout
    bounced = False
    while time.monotonic() < deadline:
        port = find_serial_port(vidpid, serial_number, timeout=0.5)
        if port:
            return port
        if not bounced and time.monotonic() > deadline - timeout / 2:
            recover_xhci_port()
            bounced = True
    who = f"vid:pid={vidpid[0]:04x}:{vidpid[1]:04x}"
    if serial_number:
        who += f" serial={serial_number}"
    raise RigError(f"serial device not found: {who}")


def wait_stm32_cdc(id_substring, timeout=25.0):
    """Wait for the F469's user-USB CDC by /dev/serial/by-id substring, with
    a one-shot xHCI controller bounce if the port wedges after flashing.
    Both gm65 firmwares and the micronuts wallet share VID:PID 16c0:27dd /
    serial F4691, so the by-id PRODUCT string is the only discriminator:
      gm65-scanner_QR_Barcode_Scanner_F4691   (this repo's firmware)
      Micronuts_Cashu_Hardware_Wallet_F4691   (the wallet image to restore)
    The async firmware is the odd one out on VID:PID (c0de:cafe)."""
    port = wait_serial_by_id_healed(id_substring, timeout)
    if port is None:
        raise RigError(f"no /dev/serial/by-id entry matching {id_substring!r}")
    return port


def stm32_cdc_identity():
    """Product substring of whatever firmware currently owns the F469's
    user USB ('gm65-scanner_QR_Barcode_Scanner' / 'Micronuts_Cashu_Hardware_
    Wallet' / 'gm65-scanner_USB' for async), or None when absent."""
    import glob as _glob

    for entry in _glob.glob("/dev/serial/by-id/*F4691*"):
        name = entry.rsplit("/", 1)[-1]
        return name[: -len("-if00")]
    return None


def cdc_with_retries(port, attempts=10, settle_s=3.0, timeout=10.0):
    """ScannerStatus until the async firmware's scanner task settles post-boot
    (CDC enumerates before the scanner task is ready — silent until ~10s)."""
    cdc = StmCdcClient(port, timeout=timeout)
    for _ in range(attempts):
        cdc.drain()
        try:
            cdc.scanner_status()
            return cdc
        except RigError:
            time.sleep(settle_s)
    cdc.close()
    raise RigError(f"STM32 CDC on {port} never answered ScannerStatus")


class CydQrClient:
    """Line-protocol client for the cyd-qr firmware (CH340 UART0)."""

    def __init__(self, port: str, timeout=8.0):
        self.ser = serial.Serial(port, 115200, timeout=timeout)
        self.ser.reset_input_buffer()

    def close(self):
        self.ser.close()

    def _cmd(self, line: str) -> str:
        self.ser.reset_input_buffer()
        self.ser.write(line.encode() + b"\n")
        self.ser.flush()
        reply = self.ser.readline().decode(errors="replace").strip()
        if not reply:
            raise RigError(f"CYD no reply to {line!r}")
        return reply

    def id(self) -> str:
        return self._cmd("ID")

    def show_qr(self, payload: bytes) -> tuple[int, int]:
        reply = self._cmd("QR " + payload.hex())
        parts = reply.split()
        if len(parts) != 4 or parts[0] != "RENDERED":
            raise RigError(f"CYD QR render failed: {reply!r} (payload {payload[:20]!r})")
        return int(parts[1]), int(parts[2])

    def clear(self) -> None:
        reply = self._cmd("CLR")
        if reply != "CLEARED":
            raise RigError(f"CYD clear failed: {reply!r}")


class StmCdcClient:
    """3-byte framed CDC client for gm65-scanner firmware ([cmd, len_hi, len_lo])."""

    def __init__(self, port: str, timeout=4.0):
        self.ser = serial.Serial(port, 115200, timeout=timeout)

    def close(self):
        self.ser.close()

    def drain(self):
        self.ser.reset_input_buffer()
        time.sleep(0.2)
        old = self.ser.timeout
        self.ser.timeout = 0.2
        while self.ser.read(256):
            pass
        self.ser.timeout = old

    def send_recv(self, cmd: int, payload: bytes = b"", timeout=4.0):
        frame = bytes([cmd, (len(payload) >> 8) & 0xFF, len(payload) & 0xFF]) + payload
        self.ser.write(frame)
        self.ser.flush()
        old = self.ser.timeout
        self.ser.timeout = timeout
        resp = self.ser.read(3)
        body = b""
        if len(resp) == 3:
            length = (resp[1] << 8) | resp[2]
            body = self.ser.read(length) if length else b""
        self.ser.timeout = old
        if len(resp) < 3:
            return None, b""
        return resp[0], body

    def scanner_status(self):
        status, payload = self.send_recv(CMD_STATUS)
        if status != STATUS_OK or len(payload) < 3:
            raise RigError(f"ScannerStatus failed: status={status} payload={payload!r}")
        return {"model": payload[0], "fw": payload[1], "connected": payload[2]}

    def trigger(self):
        status, _ = self.send_recv(CMD_TRIGGER, timeout=8.0)
        return status

    def read_data(self):
        return self.send_recv(CMD_DATA, timeout=4.0)


def _kill_port_users(port: Path):
    subprocess.run(["pkill", "-f", f"raw_logger.py {port}"], capture_output=True)
    time.sleep(0.3)
    subprocess.run(["fuser", "-k", str(port)], capture_output=True)
    time.sleep(0.7)


def flash_cyd(elf: Path, port: Path | None = None) -> None:
    require_board(CYD_REGISTRY_KEY, "flash")
    port = port or cyd_port()
    _kill_port_users(port)
    cmd = f". ~/export-esp.sh && espflash flash -p {port} --chip esp32 {elf}"
    result = subprocess.run(["bash", "-c", cmd], capture_output=True, text=True, timeout=180)
    if result.returncode != 0:
        raise RigError(f"espflash failed: {result.stderr[-800:]}")


def _cargo_bin(cmd: list[str], cwd: Path, bin_name: str) -> Path:
    """Run a cargo build (cmd must include --message-format=json) and return
    the produced bin path from message JSON (the machine shares one target-dir
    across projects via ~/.cargo/config.toml, so guessing target/ paths is
    wrong)."""
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=1800, cwd=cwd)
    if result.returncode != 0:
        raise RigError(f"cargo build failed ({bin_name}): {result.stderr[-800:]}")
    for line in result.stdout.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") != "compiler-artifact":
            continue
        if msg.get("executable") and msg.get("target", {}).get("name") == bin_name:
            return Path(msg["executable"])
    raise RigError(f"no compiler-artifact with executable for {bin_name!r}")


def build_cyd_elf() -> Path:
    crate = REPO_ROOT / "tools" / "cyd-qr"
    return _cargo_bin(
        ["bash", "-c",
         ". ~/export-esp.sh && cargo +esp build --release --message-format=json"],
        cwd=crate, bin_name="cyd-qr",
    )


def build_stm32_bin() -> Path:
    """Build the firmware under test. GM65_TEST_FW selects sync (the
    touch-gate fix lineage, buzzer-proven scan path) or async (default,
    channel-based CDC)."""
    fw = os.environ.get("GM65_TEST_FW", "async")
    if fw == "sync":
        bin_name, features = "stm32f469i-disco-scanner", "sync-mode"
    else:
        bin_name, features = "async_firmware", "scanner-async"
    manifest = REPO_ROOT / "examples" / "stm32f469i-disco" / "Cargo.toml"
    elf = _cargo_bin(
        [
            "cargo", "build", "--release", "--target", "thumbv7em-none-eabihf",
            "--manifest-path", str(manifest), "--bin", bin_name,
            "--no-default-features", "--features", features,
            "--message-format=json",
        ],
        cwd=REPO_ROOT, bin_name=bin_name,
    )
    bin_path = Path(f"/tmp/gm65-{bin_name}.bin")
    result = subprocess.run(
        ["arm-none-eabi-objcopy", "-O", "binary", str(elf), str(bin_path)],
        capture_output=True, timeout=60,
    )
    if result.returncode != 0:
        raise RigError("objcopy failed")
    return bin_path


def _st_flash(*args, timeout=300) -> subprocess.CompletedProcess:
    result = subprocess.run(
        ["st-flash", "--connect-under-reset", *args],
        capture_output=True, text=True, timeout=timeout,
    )
    if result.returncode != 0:
        raise RigError(f"st-flash {' '.join(args)} failed: {result.stderr[-500:]}")
    return result


def backup_stm32(dest: Path) -> Path:
    require_board(STM32_REGISTRY_KEY, "flash")
    _st_flash("read", str(dest), hex(STM32_FLASH_BASE), hex(STM32_FLASH_SIZE), timeout=600)
    return dest


def flash_stm32(bin_path: Path) -> None:
    require_board(STM32_REGISTRY_KEY, "flash")
    subprocess.run(["pkill", "-9", "st-flash"], capture_output=True)
    time.sleep(1)
    _st_flash("write", str(bin_path), hex(STM32_FLASH_BASE))
    # st-flash write alone leaves the target wedged on this board (USB dead
    # until an explicit reset — bench-verified 2026-09-08/09)
    _st_flash("reset", timeout=60)


def restore_stm32(backup: Path) -> None:
    flash_stm32(backup)


def st_reset() -> None:
    _st_flash("reset", timeout=60)


@dataclass
class ScanResult:
    payload: bytes
    type_byte: int
    attempts: int
    latency_s: float


def scan_until(
    cdc: StmCdcClient,
    expected: bytes | None,
    deadline_s: float = 25.0,
    retrigger_every_s: float = 5.0,
) -> ScanResult | None:
    """Trigger and poll ScannerData until a scan arrives (optionally matching
    `expected`). Returns the first scan observed, or None at the deadline."""
    start = time.monotonic()
    deadline = start + deadline_s
    last_trigger = 0.0
    attempts = 0
    while time.monotonic() < deadline:
        if time.monotonic() - last_trigger >= retrigger_every_s:
            cdc.trigger()
            last_trigger = time.monotonic()
        status, payload = cdc.read_data()
        if status == STATUS_OK and len(payload) >= 2:
            attempts += 1
            result = ScanResult(
                payload=payload[1:], type_byte=payload[0], attempts=attempts,
                latency_s=time.monotonic() - start,
            )
            if expected is None or result.payload == expected:
                return result
        time.sleep(0.4)
    return None
