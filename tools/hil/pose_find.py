"""Buzzer-assisted pose finder — productizes the /tmp/gm65_posefind2.py
pattern (issue #99).

Arms the decode buzzer (SetSettings 0xD1), renders a fresh unique QR every
few seconds, and logs every decode. A human at the bench moves the harness
slowly (tilt ±15°, sweep 8-20cm) until beeps; the pocket gets taped.

Non-mutating on the F469: runs whatever gm65 firmware is on the board.
Flashes the CYD only if it is not already running the cyd-qr firmware.
Always re-silences the module on exit (0x91) — the buzzer is the tool,
not a side effect.

Usage: python3 pose_find.py [minutes]     (default 10; Ctrl-C to stop)
"""

import sys
import time
from collections import deque
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import gm65qr
import rig
from tollgate_lab import acquire_bench_lock

POSE_PAYLOAD_LEN = 20


def find_cdc():
    import glob as _glob
    for pat in ("/dev/serial/by-id/usb-gm65-scanner*",
                "/dev/serial/by-id/*c0de*"):
        ports = _glob.glob(pat)
        if ports:
            return ports[0]
    return None


def main():
    minutes = float(sys.argv[1]) if len(sys.argv) > 1 else 10.0
    run_dir = Path(__file__).parent / "results" / f"posefind-{time.strftime('%Y%m%d-%H%M%S')}"
    run_dir.mkdir(parents=True)
    log = open(run_dir / "log", "a")

    def note(msg):
        line = f"[{time.strftime('%H:%M:%S')}] {msg}"
        print(line, flush=True)
        log.write(line + "\n")
        log.flush()

    lock = acquire_bench_lock("amperstrand-bench", project="gm65-posefind",
                              cwd=str(Path(__file__).resolve().parents[2]))
    cdc = None
    cyd = None
    seq = 0
    decodes = 0
    try:
        port = find_cdc()
        if port is None:
            note("no gm65 CDC found — is a gm65 firmware flashed on the F469?")
            return
        cdc = rig.cdc_with_retries(port)

        cyd = rig.CydQrClient(str(rig.cyd_port()))
        fw_id = cyd.id()
        if not fw_id.startswith("CYDQR"):
            note(f"CYD runs {fw_id!r} — flashing cyd-qr")
            cyd.close()
            rig.flash_cyd(rig.build_cyd_elf())
            time.sleep(2)
            cyd = rig.CydQrClient(str(rig.cyd_port()))
        gm65qr.arm_winning_config(cyd)

        # Arm the buzzer explicitly (this tool's whole point); re-silenced
        # in finally. SetSettings+readback via the rig's proven path.
        cdc.drain()
        _, _ = cdc.send_recv(rig.CMD_SET_SETTINGS, bytes([rig.SETTINGS_BUZZER]))
        cdc.drain()
        _, verify = cdc.send_recv(rig.CMD_GET_SETTINGS)
        note(f"buzzer armed (readback {verify.hex()}) — beeps = decode pocket")

        note("MOVE THE HARNESS SLOWLY: tilt ±15°, sweep 8-20cm.")
        note("When beeps start, STOP — tape the pocket exactly there.")
        deadline = time.monotonic() + minutes * 60.0
        last_diag_isr = 0
        quiet_streak = 0
        rendered = deque(maxlen=64)
        # drain pre-session residue (the module's buffered last decodes)
        for _ in range(3):
            gm65qr.consume_stale(cdc)
            time.sleep(0.3)
        while time.monotonic() < deadline:
            seq += 1
            payload = gm65qr.unique_payload(40000 + seq, POSE_PAYLOAD_LEN)
            rendered.append(payload)
            cyd.show_qr(payload)
            cdc.trigger()
            got = None
            window = time.monotonic() + 4.0
            while time.monotonic() < window:
                status, pl = cdc.read_data()
                if status == rig.STATUS_OK and len(pl) >= 2:
                    got = gm65qr.sanitize_scan(pl[1:])
                    break
                time.sleep(0.3)
            if got is not None and got in rendered:
                # delivery can lag renders by several cycles under the
                # sustained-load backlog — any THIS-session payload means
                # the module is decoding fresh renders: pocket hit
                decodes += 1
                quiet_streak = 0
                note(f"BEEP  decode #{decodes} "
                     f"{'(lagged delivery)' if got != payload else ''} — "
                     f"pocket FOUND, hold still")
            elif got is not None:
                # pre-session re-transmission; no fresh decode, no beep
                note(f"stale decode flushed (not counted): "
                     f"{got.decode(errors='replace')!r}")
            else:
                quiet_streak += 1
                note(f"quiet ({quiet_streak} in a row)")
                if quiet_streak == 10:
                    d = cdc.diagnostics()
                    isr = d.get("isr_fires")
                    if isr is not None and isr != last_diag_isr:
                        note("module UART alive but no decodes — that is the "
                             "pose, keep moving slowly")
                    last_diag_isr = isr or 0
        note(f"done: {decodes} decodes in {minutes:.0f} min "
             f"({'pocket found' if decodes else 'no pocket hit — retry with wider sweep'})")
    except KeyboardInterrupt:
        note(f"interrupted — {decodes} decodes so far")
    finally:
        if cdc is not None:
            try:
                cdc.drain()
                _, _ = cdc.send_recv(rig.CMD_SET_SETTINGS,
                                     bytes([rig.SETTINGS_SILENT]))
                cdc.drain()
                _, verify = cdc.send_recv(rig.CMD_GET_SETTINGS)
                note(f"re-silenced (readback {verify.hex()})")
            except Exception as exc:
                note(f"RE-SILENCE FAILED ({exc}) — run SetSettings(0x91)!")
            cdc.close()
        if cyd is not None:
            cyd.close()
        log.close()
        lock.release()


if __name__ == "__main__":
    main()
