"""Standalone e8 (soak_diag) runner — the #92 bisect attempt on sync.

Flashes sync firmware (which ships the rich 0x20 Diagnostic), runs the
scan loop with diagnostics polled every 5th scan until degradation or
60 scans, restores the pre-session image. The counter that spikes as
delivery dies names the failing subsystem.

Usage: python3 soak_diag.py
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import campaign
import experiments
import rig
from tollgate_lab import acquire_bench_lock


def main():
    run_dir = Path(__file__).parent / "results" / f"soakdiag-{time.strftime('%Y%m%d-%H%M%S')}"
    run_dir.mkdir(parents=True)
    log = open(run_dir / "run.log", "a")

    def note(msg):
        line = f"[{time.strftime('%H:%M:%S')}] {msg}"
        print(line, flush=True); log.write(line + "\n"); log.flush()

    lock = acquire_bench_lock("amperstrand-bench", project="gm65-soakdiag",
                              cwd=str(Path(__file__).resolve().parents[2]))
    try:
        backup = run_dir / "backup.bin"
        pre = rig.stm32_cdc_identity()
        rig.backup_stm32(backup)
        cdc, cyd = campaign.bringup_firmware("sync", log)
        note(f"baseline diagnostics: {cdc.diagnostics()}")
        t0 = time.monotonic()
        result = experiments.e8_soak_diag(cdc, cyd)
        note(f"e8 done in {time.monotonic()-t0:.0f}s: "
             f"{result['ok']}/{result['n']} scans, degraded_after={result['degraded_after']}")
        with open(run_dir / "e8.json", "w") as f:
            json.dump(result, f, indent=2, default=str)
        cdc.close(); cyd.close()
        rig.restore_stm32(backup)
        rig.wait_stm32_cdc(pre or "F4691", timeout=120)
        note("restored")
    finally:
        log.close()
        lock.release()


if __name__ == "__main__":
    main()
