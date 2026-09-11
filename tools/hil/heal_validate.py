"""ModuleReboot (0xA5, CDC 0x23) heal validation — issue #100 Tier A.

Today's wedge class (2026-09-11, twice reproduced): after sustained-load
sync traffic, the module carries state across F469 re-flashes that chokes
the ASYNC driver — fresh async boots answer the gate with uart_errors>0
and zero matching deliveries, while fresh sync boots deliver instantly.

Flow (A/B with built-in control):
  1. async bringup + gate scan    — reproduce the choke (uart_errors>0)
  2. sync bringup + gate          — sync baseline must deliver
     CDC 0x23 ModuleReboot        — [sent, responsive, model] contract
     gate again                   — sync must still deliver post-reboot
  3. async bringup + gate         — verdict: healed iff (1) failed and (3) passed
  3b. if 0x23 did not heal: CDC 0x22 FactoryReset fallback, retest async

Usage: python3 heal_validate.py
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

CMD_MODULE_REBOOT = 0x23
CMD_FACTORY_RESET = 0x22


def gate(cdc, cyd, tag, note, deadline_s=20.0):
    r = gm65qr.scan_roundtrip(cdc, cyd, f"healval-{tag}".encode(),
                              deadline_s=deadline_s)
    diag = None
    try:
        diag = cdc.diagnostics()
    except Exception as exc:
        note(f"diag unavailable: {exc}")
    note(f"gate {tag}: {'OK' if r['ok'] else 'FAIL'} in {r['latency_s']}s diag={diag}")
    return r["ok"], diag


def main():
    run_dir = Path(__file__).parent / "results" / f"healval-{time.strftime('%Y%m%d-%H%M%S')}"
    run_dir.mkdir(parents=True)
    log = open(run_dir / "run.log", "a")

    def note(msg):
        line = f"[{time.strftime('%H:%M:%S')}] {msg}"
        print(line, flush=True)
        log.write(line + "\n")
        log.flush()

    record = {}
    lock = acquire_bench_lock("amperstrand-bench", project="gm65-healval",
                              cwd=str(Path(__file__).resolve().parents[2]))
    backup = None
    pre = None
    try:
        backup = run_dir / "backup.bin"
        pre = rig.stm32_cdc_identity()
        rig.backup_stm32(backup)

        note("phase 1: async bringup — reproduce the post-load choke")
        cdc, cyd = campaign.bringup_firmware("async", log)
        record["async_pre_heal"] = dict(zip(("ok", "diag"),
                                            gate(cdc, cyd, "pre", note)))
        cdc.close()
        cyd.close()

        note("phase 2: sync bringup — baseline + CDC 0x23 ModuleReboot")
        cdc, cyd = campaign.bringup_firmware("sync", log)
        record["sync_baseline"] = dict(zip(("ok", "diag"),
                                           gate(cdc, cyd, "syncbase", note)))
        status, payload = cdc.send_recv(CMD_MODULE_REBOOT, timeout=10.0)
        record["module_reboot"] = {
            "status": status, "payload": list(payload or b""),
            # payload contract: [reboot_sent, responsive_after, model]
            "sent": bool(payload and payload[0]),
            "responsive": bool(payload and payload[1]),
        }
        note(f"0x23 ModuleReboot: {record['module_reboot']}")
        time.sleep(3)
        record["sync_post_reboot"] = dict(zip(("ok", "diag"),
                                              gate(cdc, cyd, "syncpost", note)))
        cdc.close()
        cyd.close()

        note("phase 3: async bringup — heal verdict")
        cdc, cyd = campaign.bringup_firmware("async", log)
        record["async_post_heal"] = dict(zip(("ok", "diag"),
                                             gate(cdc, cyd, "post", note)))

        repro = not record["async_pre_heal"]["ok"]
        healed = record["async_post_heal"]["ok"]
        record["verdict_0x23"] = {
            "reproduced_choke": repro,
            "healed": healed,
            "conclusive": repro and healed,
        }
        note(f"0x23 verdict: {record['verdict_0x23']}")

        if repro and not healed:
            note("0x23 did not heal — falling back to 0x22 FactoryReset")
            cdc.close()
            cyd.close()
            cdc, cyd = campaign.bringup_firmware("sync", log)
            status, payload = cdc.send_recv(CMD_FACTORY_RESET, timeout=20.0)
            record["factory_reset"] = {
                "status": status, "payload": list(payload or b""),
                "accepted": bool(payload and payload[0]),
                "reinit_ok": bool(payload and payload[1]),
            }
            note(f"0x22 FactoryReset: {record['factory_reset']}")
            time.sleep(3)
            cdc.close()
            cyd.close()
            cdc, cyd = campaign.bringup_firmware("async", log)
            record["async_post_0x22"] = dict(zip(("ok", "diag"),
                                                 gate(cdc, cyd, "post22", note)))
            record["verdict_0x22"] = {
                "healed": record["async_post_0x22"]["ok"],
                "conclusive": record["async_post_0x22"]["ok"],
            }
            note(f"0x22 verdict: {record['verdict_0x22']}")

        cdc.close()
        cyd.close()
        (run_dir / "report.json").write_text(json.dumps(record, indent=2,
                                                         default=str))
        note(f"report: {run_dir / 'report.json'}")
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
