"""UR multi-part fragment E2E — the >92-byte path through real hardware.

The envelope ceiling is 92 bytes/frame, which is exactly why UR framing
exists: long content travels as sequential small fragments. This runner
pushes a realistic ur:bytes fragment sequence through the rig and asserts
each fragment round-trips byte-exact — the hardware story micronuts-style
consumers depend on (firmware-side reassembly is covered by lib tests;
this proves the transport).

First-run lesson (2026-09-11, sync, pose in-pocket): back-to-back
fragments hit the sustained-load delivery stall (#92/#93) — 2/3 then 0/3.
Mitigations, both campaign-proven: a SetSettings resync between fragments
(sync's stop_scan+readback resyncs the UART path) and a verify-gate first
scan with SWD-reset rebringup (a failed first scan flips sync self-healing
into continuous mode and kills the run).

Usage: python3 ur_e2e.py [async|sync]
  fw arg  — backup, flash that firmware, run, restore (soak_diag pattern)
  no arg  — run on whatever firmware is on the board
"""

import json
import sys
import time
import traceback
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import campaign
import gm65qr
import rig
from tollgate_lab import acquire_bench_lock

# ur:bytes-style fragments (BC32-ish payload, ~80B each — inside the
# 92-byte envelope, outside trivial-single-QR territory)
SEQUENCE = [
    b"ur:bytes/1-3/gdonydafwtrrnhfcajkbvtlydhdwevtylfddpewsjzoymd",
    b"ur:bytes/2-3/fpsvtvqqrgehlstlgscfejtlbjpscbfyokzkstvpaeepkgzjtaoyaea",
    b"ur:bytes/3-3/lgudhsdyolyosfnjzbetpluveoxwevwnldtssxewrsfnfxgrbb",
]

# sync's auto-scan re-arm cadence is ~5.6s/scan — deadlines must fit it
SINGLE_DEADLINE_S = 25.0
SEQUENCE_DEADLINE_S = 20.0


def resync(cdc, note):
    # SetSettings resync between fragments: sync's write+readback path
    # resyncs the UART delivery path (#92/#93 anti-stall); no-op class
    # on async. Never arms the buzzer (arm_scanner_settings honors
    # GM65_BUZZER, default silent).
    rig.arm_scanner_settings(cdc)
    note("settings resync ok")


def run_suite(cdc, cyd, note):
    results = []
    for i, frag in enumerate(SEQUENCE, 1):
        r = gm65qr.scan_roundtrip(cdc, cyd, frag, deadline_s=SINGLE_DEADLINE_S)
        results.append({"fragment": i, "len": len(frag), "ok": r["ok"],
                        "latency_s": r["latency_s"],
                        "exact": r["scanned"] == frag if r["ok"] else False})
        note(f"fragment {i}/3 ({len(frag)}B): "
             f"{'OK' if r['ok'] and results[-1]['exact'] else 'FAIL'} "
             f"in {r['latency_s']}s")
        resync(cdc, note)

    seq_start = time.monotonic()
    seq_ok = 0
    for i, frag in enumerate(SEQUENCE, 1):
        r = gm65qr.scan_roundtrip(cdc, cyd, frag, deadline_s=SEQUENCE_DEADLINE_S)
        if r["ok"] and r["scanned"] == frag:
            seq_ok += 1
        note(f"sequence {i}/3: {'OK' if r['ok'] and r['scanned'] == frag else 'FAIL'}")
        resync(cdc, note)
    return {
        "single": {"ok": sum(1 for r in results if r["ok"] and r["exact"]),
                   "n": len(results)},
        "sequence": {"ok": seq_ok, "n": len(SEQUENCE),
                     "wall_s": round(time.monotonic() - seq_start, 1)},
        "results": results,
    }


def main():
    fw = sys.argv[1] if len(sys.argv) > 1 else None
    run_dir = Path(__file__).parent / "results" / f"ure2e-{time.strftime('%Y%m%d-%H%M%S')}"
    run_dir.mkdir(parents=True)
    log = open(run_dir / "run.log", "a")

    def note(msg):
        line = f"[{time.strftime('%H:%M:%S')}] {msg}"
        print(line, flush=True)
        log.write(line + "\n")
        log.flush()

    lock = acquire_bench_lock("amperstrand-bench", project="gm65-ur-e2e",
                              cwd=str(Path(__file__).resolve().parents[2]))
    backup = None
    pre = None
    cdc = None
    cyd = None
    try:
        if fw:
            backup = run_dir / "backup.bin"
            pre = rig.stm32_cdc_identity()
            rig.backup_stm32(backup)
            cdc, cyd = campaign.bringup_firmware(fw, log)
        else:
            import glob as _glob
            port = (_glob.glob("/dev/serial/by-id/usb-gm65-scanner*")
                    or _glob.glob("/dev/serial/by-id/*c0de*"))[0]
            cdc = rig.cdc_with_retries(port)
            rig.arm_scanner_settings(cdc)
            cyd = rig.CydQrClient(str(rig.cyd_port()))
            gm65qr.arm_winning_config(cyd)

        # Verify-gate: a failed first scan flips sync self-healing into
        # continuous mode and kills the run (#93) — retry once via SWD reset.
        gate = gm65qr.scan_roundtrip(cdc, cyd, b"ur-gate-01", deadline_s=20.0)
        if not gate["ok"] and fw:
            note("gate scan FAILED — SWD reset + rebringup")
            cdc.close()
            cyd.close()
            rig.st_reset()
            time.sleep(8)
            cdc, cyd = campaign.bringup_firmware(fw, log)
            gate = gm65qr.scan_roundtrip(cdc, cyd, b"ur-gate-02", deadline_s=20.0)
        if not gate["ok"]:
            try:
                note(f"gate-failure diagnostics: {cdc.diagnostics()}")
            except Exception as exc:
                note(f"gate-failure diagnostics unavailable: {exc}")
            note("gate scan failed — module not delivering; aborting")
            (run_dir / "ABORTED.txt").write_text("gate scan failed\n")
            return
        note("gate scan OK — fragments starting")

        summary = run_suite(cdc, cyd, note)
        summary["fw"] = fw or "on-board"
        (run_dir / "summary.json").write_text(json.dumps(summary, indent=2))
        note(f"summary: single {summary['single']['ok']}/{summary['single']['n']}, "
             f"sequence {summary['sequence']['ok']}/{summary['sequence']['n']} "
             f"in {summary['sequence']['wall_s']}s")
    except Exception:
        note("EXCEPTION:\n" + traceback.format_exc())
    finally:
        for client in (cdc, cyd):
            try:
                if client:
                    client.close()
            except Exception:
                pass
        if backup is not None and backup.exists():
            try:
                rig.restore_stm32(backup)
                rig.wait_stm32_cdc(pre or "F4691", timeout=120)
                note("restored")
            except Exception:
                note("RESTORE FAILED — board may hold test firmware")
        log.close()
        lock.release()


if __name__ == "__main__":
    main()
