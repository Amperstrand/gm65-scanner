"""Standalone e8 (soak_diag) runner — the #92 bisect attempt.

Flashes the firmware under test, runs the envelope-ladder scan loop with
diagnostics polled every 5th scan until degradation or 60 scans, restores.
Every scan is logged live so partial data survives a crash; exceptions are
captured into run.log (stdout goes nowhere on setsid runs).

Usage: python3 soak_diag.py [sync|async]
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


def main():
    fw = sys.argv[1] if len(sys.argv) > 1 else "sync"
    run_dir = Path(__file__).parent / "results" / f"soakdiag-{time.strftime('%Y%m%d-%H%M%S')}"
    run_dir.mkdir(parents=True)
    log = open(run_dir / "run.log", "a")

    def note(msg):
        line = f"[{time.strftime('%H:%M:%S')}] {msg}"
        print(line, flush=True)
        log.write(line + "\n")
        log.flush()

    lock = acquire_bench_lock("amperstrand-bench", project="gm65-soakdiag",
                              cwd=str(Path(__file__).resolve().parents[2]))
    backup = None
    pre = None
    try:
        backup = run_dir / "backup.bin"
        pre = rig.stm32_cdc_identity()
        rig.backup_stm32(backup)
        cdc, cyd = campaign.bringup_firmware(fw, log)
        baseline = cdc.diagnostics()
        note(f"baseline: {baseline}")

        # Verify-scan gate: a single failed first scan flips the sync
        # self-healing into continuous/induction mode and kills the whole
        # run (#93, live-captured 2026-09-10). Retry once via SWD reset.
        gate = gm65qr.scan_roundtrip(cdc, cyd, b"gate-01", deadline_s=20.0)
        if not gate["ok"]:
            note("gate scan FAILED — SWD reset + rebringup")
            rig.st_reset()
            time.sleep(8)
            port = rig.wait_stm32_cdc("gm65-scanner", timeout=45)
            cdc = rig.cdc_with_retries(port)
            rig.arm_scanner_settings(cdc)
            gm65qr.arm_winning_config(cyd)
            gate = gm65qr.scan_roundtrip(cdc, cyd, b"gate-02", deadline_s=20.0)
            if not gate["ok"]:
                note("gate scan failed twice — module likely wedged "
                     "(needs power cycle or factory-reset 0x00D9); aborting")
                (run_dir / "ABORTED.txt").write_text(
                    "gate scan failed after SWD-reset retry\n")
                cdc.close()
                cyd.close()
                return
        note("gate scan OK — ladder starting")

        sizes = [7, 10, 14, 24, 34, 44, 58, 72, 92]
        rows = []
        for i in range(60):
            payload = gm65qr.unique_payload(9000 + i, sizes[i % len(sizes)])
            r = gm65qr.scan_roundtrip(cdc, cyd, payload, deadline_s=15.0)
            row = {"i": i, "size": sizes[i % len(sizes)], "ok": r["ok"],
                   "latency_s": r["latency_s"]}
            line = (f"scan {i:2d} size={row['size']:2d} "
                    f"{'OK  ' if r['ok'] else 'FAIL'} "
                    f"{r['latency_s'] if r['latency_s'] is not None else '-'}s")
            if i % 5 == 0:
                try:
                    row["diag"] = cdc.diagnostics()
                    line += f" diag={row['diag']}"
                except Exception as exc:
                    line += f" diag-ERROR: {exc}"
            note(line)
            rows.append(row)
        final = cdc.diagnostics()
        oks = sum(1 for r in rows if r["ok"])
        result = {"fw": fw, "baseline": baseline, "final": final, "rows": rows,
                  "ok": oks, "n": len(rows),
                  "degraded_after": next((r["i"] for r in rows if not r["ok"]), None)}
        (run_dir / "e8.json").write_text(json.dumps(result, indent=2, default=str))
        note(f"e8 done: {oks}/{len(rows)}, degraded_after={result['degraded_after']}")
        cdc.close()
        cyd.close()
    except Exception:
        note("EXCEPTION:\n" + traceback.format_exc())
    finally:
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
