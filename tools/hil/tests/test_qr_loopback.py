"""CYD → GM65 QR loopback scenarios (flash-mutation, opt-in).

Session fixture `qr_rig` performs the destructive phases and ALWAYS restores
the micronuts wallet firmware image (2 MB flash backup) on teardown.

Run:
    cd tools/hil && python3 -m pytest tests -m "hardware and flash_mutation" -v
"""

import json
import subprocess
import time

import pytest

import rig

PAYLOADS = [
    "hello-gm65",
    "https://amperstrand.no/tollgate?src=cyd&run=1",
    "QR-RIG-STRESS-0123456789-0123456789-0123456789-0123456789-0123456789"
    "-0123456789-0123456789-0123456789-0123456789-0123456789-END",
]


@pytest.fixture(scope="session")
def qr_rig(rig_lock, artifacts_dir):
    rig.require_board(rig.CYD_REGISTRY_KEY, "flash")
    rig.require_board(rig.STM32_REGISTRY_KEY, "flash")

    (verdicts := artifacts_dir / "verdicts.json").write_text("[]")
    backup_path = artifacts_dir / "stm32-wallet-backup.bin"

    cyd = None
    cdc = None
    try:
        # CYD: build, flash, verify line protocol
        elf = rig.build_cyd_elf()
        rig.flash_cyd(elf)
        time.sleep(2)
        cyd = rig.CydQrClient(str(rig.cyd_port()))
        fw_id = cyd.id()
        assert fw_id.startswith("CYDQR"), f"unexpected CYD firmware id: {fw_id!r}"

        # F469: backup wallet image, flash gm65 async firmware, verify CDC + GM65
        rig.backup_stm32(backup_path)
        bin_path = rig.build_stm32_bin()
        rig.flash_stm32(bin_path)
        port = rig.wait_serial_port(rig.GM65_CDC_VIDPID, timeout=25)
        time.sleep(3)
        cdc = rig.cdc_with_retries(port)
        status = cdc.scanner_status()
        assert status["connected"] == 1, f"GM65 not connected: {status}"

        yield {"cyd": cyd, "cdc": cdc, "artifacts": artifacts_dir,
               "verdicts": verdicts, "backup": backup_path}
    finally:
        results = {"restored": False}
        try:
            if cdc:
                cdc.close()
            if cyd:
                cyd.close()
            rig.restore_stm32(backup_path)
            # wallet identity: product string, NOT VID:PID/serial (gm65 sync
            # firmware shares both with the wallet)
            rig.wait_stm32_cdc("Micronuts_Cashu_Hardware_Wallet", timeout=120)
            results["restored"] = True
        finally:
            record = json.loads(verdicts.read_text() or "[]")
            record.append({"phase": "restore-wallet",
                           "restored": results["restored"], "ok": results["restored"]})
            verdicts.write_text(json.dumps(record, indent=2))
            _note_place(firmware="micronuts-wallet-restored" if results["restored"]
                        else "RESTORE-FAILED")


def _note_place(**tags):
    args = [f"{k}={v}" for k, v in tags.items()]
    args.append(f"ts={time.strftime('%Y%m%dT%H%M%S')}")
    subprocess.run(
        ["labgrid-client", "-x", "192.168.13.221:20408", "-p", "gm65-qr-loopback",
         "set-tags", *args],
        capture_output=True, timeout=10,
    )


def _record(qr_rig, entry):
    path = qr_rig["verdicts"]
    record = json.loads(path.read_text() or "[]")
    record.append(entry)
    path.write_text(json.dumps(record, indent=2))


@pytest.mark.hardware
@pytest.mark.flash_mutation
def test_cyd_firmware_up(qr_rig):
    fw_id = qr_rig["cyd"].id()
    assert fw_id.startswith("CYDQR"), fw_id
    _record(qr_rig, {"test": "cyd_firmware_up", "id": fw_id, "ok": True})


@pytest.mark.hardware
@pytest.mark.flash_mutation
def test_scanner_connected(qr_rig):
    status = qr_rig["cdc"].scanner_status()
    assert status["connected"] == 1, status
    _record(qr_rig, {"test": "scanner_connected", "status": status, "ok": True})


@pytest.mark.hardware
@pytest.mark.flash_mutation
@pytest.mark.parametrize("payload", PAYLOADS, ids=lambda p: f"{len(p)}B")
def test_qr_roundtrip(qr_rig, payload):
    expected = payload.encode()
    modules, scale = qr_rig["cyd"].show_qr(expected)
    scan = rig.scan_until(qr_rig["cdc"], expected, deadline_s=30.0)
    ok = scan is not None
    _record(qr_rig, {
        "test": "qr_roundtrip", "payload": payload,
        "qr_modules": modules, "px_per_module": scale,
        "scanned": scan.payload.decode(errors="replace") if scan else None,
        "latency_s": round(scan.latency_s, 2) if scan else None,
        "ok": ok,
    })
    assert scan is not None, f"no scan within 30s for payload {payload!r}"
    assert scan.payload == expected
    assert scan.type_byte == 0x00, f"expected Text/URL type, got 0x{scan.type_byte:02x}"


@pytest.mark.hardware
@pytest.mark.flash_mutation
def test_blank_screen_scans_nothing(qr_rig):
    """Negative control: blanked CYD must not yield any of our payloads.
    Ambient barcodes are tolerated (logged, not asserted on)."""
    qr_rig["cyd"].clear()
    scan = rig.scan_until(qr_rig["cdc"], None, deadline_s=8.0)
    ambient = scan.payload.decode(errors="replace") if scan else None
    _record(qr_rig, {
        "test": "blank_screen_negative", "ambient_scan": ambient,
        "clean": scan is None, "ok": scan is None or scan.payload not in
        [p.encode() for p in PAYLOADS],
    })
    assert scan is None or scan.payload not in [p.encode() for p in PAYLOADS], (
        f"stale scan matched a test payload on a blank screen: {ambient!r}"
    )
