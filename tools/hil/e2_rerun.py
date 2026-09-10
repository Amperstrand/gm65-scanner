"""Standalone E2 (QR envelope) rerun — fixed ladder (<=240B, see LINE_MAX
finding). Runs on both firmwares with the campaign's bringup/restore flow.
Usage: python3 e2_rerun.py [async|sync]
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
    fw = sys.argv[1] if len(sys.argv) > 1 else "async,sync"
    run_dir = Path(__file__).parent / "results" / f"e2rerun-{time.strftime('%Y%m%d-%H%M%S')}"
    run_dir.mkdir(parents=True)
    log = open(run_dir / "run.log", "a")

    def note(msg):
        line = f"[{time.strftime('%H:%M:%S')}] {msg}"
        print(line, flush=True); log.write(line + "\n"); log.flush()

    lock = acquire_bench_lock("amperstrand-bench", project="gm65-e2rerun",
                              cwd=str(Path(__file__).resolve().parents[2]))
    try:
        backup = run_dir / "backup.bin"
        pre = rig.stm32_cdc_identity()
        rig.backup_stm32(backup)
        for fw_name in fw.split(","):
            campaign.place_tags(fw_name, "e2-rerun")
            try:
                cdc, cyd = campaign.bringup_firmware(fw_name, log)
                t0 = time.monotonic()
                result = experiments.e2_qr_envelope(cdc, cyd)
                note(f"{fw_name}: e2 done in {time.monotonic()-t0:.0f}s "
                     f"({result['ok']}/{result['total']} cells)")
                with open(run_dir / f"{fw_name}-e2.json", "w") as f:
                    json.dump(result, f, indent=2, default=str)
                cdc.close(); cyd.close()
            except Exception as exc:
                note(f"{fw_name}: e2 FAILED: {exc}")
        rig.restore_stm32(backup)
        rig.wait_stm32_cdc(pre or "F4691", timeout=120)
        note("restored")
    finally:
        log.close()
        lock.release()


if __name__ == "__main__":
    main()
