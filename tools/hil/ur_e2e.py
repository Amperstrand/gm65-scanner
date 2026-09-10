"""UR multi-part fragment E2E — the >92-byte path through real hardware.

The envelope ceiling is 92 bytes/frame, which is exactly why UR framing
exists: long content travels as sequential small fragments. This runner
pushes a realistic ur:bytes fragment sequence through the rig and asserts
each fragment round-trips byte-exact — the hardware story micronuts-style
consumers depend on (firmware-side reassembly is covered by lib tests;
this proves the transport).

Usage: python3 ur_e2e.py   (takes the flock; expects a flashed firmware,
runs on whatever is on the board — use after the campaign's restore)
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

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


def main():
    run_dir = Path(__file__).parent / "results" / f"ure2e-{time.strftime('%Y%m%d-%H%M%S')}"
    run_dir.mkdir(parents=True)
    lock = acquire_bench_lock("amperstrand-bench", project="gm65-ur-e2e",
                              cwd=str(Path(__file__).resolve().parents[2]))
    try:
        import glob as _glob
        port = (_glob.glob("/dev/serial/by-id/usb-gm65-scanner*")
                or _glob.glob("/dev/serial/by-id/*c0de*"))[0]
        cdc = rig.cdc_with_retries(port)
        rig.arm_scanner_settings(cdc)
        cyd = rig.CydQrClient(str(rig.cyd_port()))
        gm65qr.arm_winning_config(cyd)

        results = []
        for i, frag in enumerate(SEQUENCE, 1):
            r = gm65qr.scan_roundtrip(cdc, cyd, frag, deadline_s=20.0)
            results.append({"fragment": i, "len": len(frag), "ok": r["ok"],
                            "latency_s": r["latency_s"],
                            "exact": r["scanned"] == frag if r["ok"] else False})
            print(f"fragment {i}/3 ({len(frag)}B): "
                  f"{'OK' if r['ok'] and results[-1]['exact'] else 'FAIL'} "
                  f"in {r['latency_s']}s")

        # rapid-sequence pass: fragments back-to-back (the wallet flow)
        seq_start = time.monotonic()
        seq_ok = 0
        for frag in SEQUENCE:
            r = gm65qr.scan_roundtrip(cdc, cyd, frag, deadline_s=15.0)
            if r["ok"] and r["scanned"] == frag:
                seq_ok += 1
        summary = {
            "single": {"ok": sum(1 for r in results if r["ok"] and r["exact"]),
                       "n": len(results)},
            "sequence": {"ok": seq_ok, "n": len(SEQUENCE),
                         "wall_s": round(time.monotonic() - seq_start, 1)},
            "results": results,
        }
        (run_dir / "summary.json").write_text(json.dumps(summary, indent=2))
        print(f"summary: single {summary['single']['ok']}/{summary['single']['n']}, "
              f"sequence {seq_ok}/{len(SEQUENCE)} in {summary['sequence']['wall_s']}s")
        cdc.close(); cyd.close()
    finally:
        lock.release()


if __name__ == "__main__":
    main()
