"""Campaign runner: full sync+async characterization in one bench session.

One flock hold, one CYD flash, one STM32 backup; per-firmware flash cycles
between experiment batches. Every experiment is fault-isolated — a wedge
records and the campaign continues. Artifacts land in
tools/hil/results/campaign-<ts>/ (summary.md + per-experiment JSON).

Usage: python3 campaign.py [--firmwares async,sync] [--quick]
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import sys
import time
import traceback
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import experiments
import gm65qr
import rig
from tollgate_lab import BenchLockHeldError, acquire_bench_lock

COORDINATOR = "192.168.13.221:20408"
PLACE = "gm65-qr-loopback"

EXPERIMENTS = [
    ("e1_reliability_soak", lambda cdc, cyd: experiments.e1_reliability_soak(cdc, cyd)),
    ("e2_qr_envelope", lambda cdc, cyd: experiments.e2_qr_envelope(cdc, cyd)),
    ("e3_scan_speed", lambda cdc, cyd: experiments.e3_scan_speed(cdc, cyd)),
    ("e4_settings_ab", lambda cdc, cyd: experiments.e4_settings_ab(cdc, cyd)),
    ("e5_negative_controls", lambda cdc, cyd: experiments.e5_negative_controls(cdc, cyd)),
    ("e6_wedge_repro", experiments.e6_wedge_repro),
    ("e7_jitter_net", lambda cdc, cyd: experiments.e7_jitter_net(cdc, cyd)),
]

QUICK_SUBSET = ["e1_reliability_soak", "e5_negative_controls"]


def note(log, msg):
    line = f"[{time.strftime('%H:%M:%S')}] {msg}"
    print(line, flush=True)
    log.write(line + "\n")
    log.flush()


def place_tags(firmware, status):
    """Best-effort state record on the labgrid place — never fatal (the
    coordinator went unresponsive mid-campaign once and must not kill runs)."""
    import subprocess
    try:
        subprocess.run(
            ["labgrid-client", "-x", COORDINATOR, "-p", PLACE, "set-tags",
             f"firmware={firmware}", f"test=campaign:{status}",
             "owner=gm65-scanner", f"ts={time.strftime('%Y%m%dT%H%M%S')}"],
            capture_output=True, timeout=5)
    except Exception:
        pass


def bringup_firmware(fw, log):
    """Flash + heal + settle, return (cdc, cyd) or raise."""
    os.environ["GM65_TEST_FW"] = fw
    bin_path = rig.build_stm32_bin()
    rig.flash_stm32(bin_path)
    if fw == "async":
        port = rig.wait_serial_port(rig.GM65_CDC_VIDPID, timeout=45)
    else:
        port = rig.wait_stm32_cdc("gm65-scanner", timeout=45)
    time.sleep(3)
    cdc = rig.cdc_with_retries(port)
    rig.arm_scanner_settings(cdc)  # silent 0x91 unless GM65_BUZZER=1
    elf = rig.build_cyd_elf()
    rig.flash_cyd(elf)
    time.sleep(2)
    cyd = rig.CydQrClient(str(rig.cyd_port()))
    assert cyd.id().startswith("CYDQR")
    gm65qr.arm_winning_config(cyd)
    note(log, f"{fw}: CDC={port}, scanner={cdc.scanner_status()}, winning config armed")
    return cdc, cyd


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--firmwares", default="async,sync")
    parser.add_argument("--quick", action="store_true",
                        help="reliability + negatives only (fast smoke)")
    args = parser.parse_args()

    run_dir = Path(__file__).parent / "results" / f"campaign-{time.strftime('%Y%m%d-%H%M%S')}"
    run_dir.mkdir(parents=True)
    log = open(run_dir / "campaign.log", "a")

    try:
        lock = acquire_bench_lock("amperstrand-bench", project="gm65-campaign",
                                  cwd=str(Path(__file__).resolve().parents[2]))
    except BenchLockHeldError as exc:
        note(log, f"FATAL: bench flock held: {exc}")
        return 3

    summary = {"started": time.strftime("%Y-%m-%dT%H:%M:%S"), "firmwares": {}}
    backup = run_dir / "stm32-firmware-backup.bin"
    pre_identity = rig.stm32_cdc_identity()
    try:
        note(log, f"campaign start; pre-session identity: {pre_identity}")
        place_tags("-", "starting")
        rig.backup_stm32(backup)
        note(log, f"backup: {backup} ({backup.stat().st_size} bytes)")

        selected = [e for e in EXPERIMENTS
                    if not args.quick or e[0] in QUICK_SUBSET]

        for fw in args.firmwares.split(","):
            fw_results = {}
            summary["firmwares"][fw] = fw_results  # live-updated, fatal-safe
            place_tags(fw, "running")
            try:
                cdc, cyd = bringup_firmware(fw, log)
            except Exception:
                fw_results["bringup_error"] = traceback.format_exc(limit=3)
                summary["firmwares"][fw] = fw_results
                note(log, f"{fw}: BRINGUP FAILED — continuing")
                continue

            for name, fn in selected:
                place_tags(fw, name)
                t0 = time.monotonic()
                try:
                    result = fn(cdc, cyd)
                    err = None
                except Exception:
                    result = None
                    err = traceback.format_exc(limit=3)
                fw_results[name] = {"duration_s": round(time.monotonic() - t0, 1),
                                    "result": result, "error": err}
                status = "ERROR" if err else "ok"
                note(log, f"{fw}/{name}: {status} ({fw_results[name]['duration_s']}s)")
                if name == "e6_wedge_repro" and result and result.get("recovered_via_swd_reset"):
                    # CDC moved to a fresh enumeration after SWD reset
                    note(log, f"{fw}: e6 recovered the board — rebinding clients")
                    cdc.close()
                    cyd.close()
                    cdc, cyd = bringup_firmware(fw, log)
                with open(run_dir / f"{fw}-{name}.json", "w") as f:
                    json.dump(fw_results[name], f, indent=2, default=str)
            try:
                cdc.close()
                cyd.close()
            except Exception:
                pass

        # restore whatever image owned the board before the campaign
        try:
            rig.restore_stm32(backup)
            expected = pre_identity or "F4691"
            rig.wait_stm32_cdc(expected, timeout=120)
            summary["restored"] = True
            note(log, f"restored pre-session image: {expected}")
        except Exception:
            summary["restore_error"] = traceback.format_exc(limit=3)
            note(log, "RESTORE FAILED — board may hold test firmware!")
    except Exception:
        summary["fatal"] = traceback.format_exc(limit=5)
        note(log, "FATAL during campaign — attempting restore")
        try:
            rig.restore_stm32(backup)
            summary["restored"] = "unverified (fatal path)"
        except Exception:
            note(log, "RESTORE FAILED — board may hold test firmware!")
    finally:
        place_tags("restored" if summary.get("restored") else "CHECK-BOARD",
                   "done")
        summary["finished"] = time.strftime("%Y-%m-%dT%H:%M:%S")
        with open(run_dir / "summary.json", "w") as f:
            json.dump(summary, f, indent=2, default=str)
        write_markdown(run_dir, summary)
        note(log, f"campaign done: {run_dir}")
        log.close()
        lock.release()
    return 0


def write_markdown(run_dir: Path, summary: dict):
    lines = [f"# QR loopback campaign — {summary['started']}", ""]
    for fw, results in summary.get("firmwares", {}).items():
        lines.append(f"## {fw}")
        for name, entry in results.items():
            if name == "bringup_error":
                lines.append(f"- bringup FAILED: `{entry}`")
                continue
            r = entry.get("result") or {}
            tag = "ERROR" if entry.get("error") else "ok"
            lines.append(f"- **{name}** [{tag}, {entry['duration_s']}s]"
                         f" {json.dumps(r, default=str)[:300]}")
        lines.append("")
    lines.append(f"restored: {summary.get('restored')}")
    (run_dir / "summary.md").write_text("\n".join(lines))


if __name__ == "__main__":
    sys.exit(main())
