"""Experiment battery for the CYD->GM65 loopback campaign.

Each experiment returns a JSON-serializable dict. Fault isolation is the
campaign runner's job — these functions assume a healthy rig and measure.
Timings assume the winning config decodes in ~1-9s (bench 2026-09-10).
"""

from __future__ import annotations

import random
import time

import gm65qr
import rig

# QR payload-size ladder (byte mode): crosses every version boundary up to
# ~240 bytes — the CYD firmware's line buffer rejects 249+ byte payloads
# (LINE_MAX off-by-one, bench 2026-09-10).
LADDER = [7, 10, 14, 24, 34, 44, 58, 72, 92, 116, 146, 180, 220, 240]
CAPS = [224, 192, 160]


def e1_reliability_soak(cdc, cyd, n=40):
    """Success rate + latency distribution at the winning config."""
    results = []
    for i in range(n):
        payload = gm65qr.unique_payload(1000 + i, 10)
        results.append(gm65qr.scan_roundtrip(cdc, cyd, payload))
    oks = [r for r in results if r["ok"]]
    lats = sorted(r["latency_s"] for r in oks)
    return {
        "n": n,
        "ok": len(oks),
        "success_rate": round(len(oks) / n, 3),
        "latency_min_s": lats[0] if lats else None,
        "latency_p50_s": lats[len(lats) // 2] if lats else None,
        "latency_p95_s": lats[int(len(lats) * 0.95)] if lats else None,
        "latency_max_s": lats[-1] if lats else None,
        "failures": [r for r in results if not r["ok"]],
    }


def e2_qr_envelope(cdc, cyd):
    """Decode matrix across payload size x cap x ECC: the QR envelope."""
    cells = []
    for size in LADDER:
        for cap in CAPS:
            cyd.set_ecch(True)
            payload = gm65qr.unique_payload(2000 + size, size)
            r = gm65qr.scan_roundtrip(cdc, cyd, payload, cap=cap, deadline_s=12.0)
            cells.append({"size": size, "cap": cap, "ecc": "H",
                          "modules": r["modules"], "px_per_module": r["px_per_module"],
                          "ok": r["ok"], "latency_s": r["latency_s"]})
            if not r["ok"]:
                # one retry — pose jitter, not envelope, may be the cause
                r2 = gm65qr.scan_roundtrip(cdc, cyd, payload, cap=cap, deadline_s=8.0)
                cells[-1].update(retry_ok=r2["ok"], retry_latency_s=r2["latency_s"])
        # ECC-M comparison subset: first + middle + last ladder size at cap 224
        if size in (LADDER[0], LADDER[len(LADDER) // 2], LADDER[-1]):
            cyd.set_ecch(False)
            payload = gm65qr.unique_payload(3000 + size, size)
            r = gm65qr.scan_roundtrip(cdc, cyd, payload, cap=224, deadline_s=12.0)
            cells.append({"size": size, "cap": 224, "ecc": "M",
                          "modules": r["modules"], "px_per_module": r["px_per_module"],
                          "ok": r["ok"], "latency_s": r["latency_s"]})
            cyd.set_ecch(True)
    return {"cells": cells,
            "ok": sum(1 for c in cells if c["ok"]),
            "total": len(cells)}


def e3_scan_speed(cdc, cyd):
    """Sustained decode cycle time + poll-cadence effect."""
    out = {"sustained": {}, "cadences": {}}
    seq = 0
    # sustained: 30 consecutive unique decodes, poll 0.3s
    start = time.monotonic()
    latencies = []
    for _ in range(30):
        seq += 1
        r = gm65qr.scan_roundtrip(cdc, cyd, gm65qr.unique_payload(4000 + seq, 10),
                                  deadline_s=25.0, poll_s=0.3)
        if not r["ok"]:
            out["sustained"]["failure_at"] = seq
            break
        latencies.append(r["latency_s"])
    total = time.monotonic() - start
    out["sustained"].update({
        "decodes": len(latencies), "wall_s": round(total, 1),
        "throughput_per_min": round(len(latencies) / total * 60, 1) if total else None,
        "per_scan_s_avg": round(total / len(latencies), 2) if latencies else None,
        "latencies": latencies,
    })
    # cadence effect: 8 decodes at each poll interval
    for poll_s in (0.2, 0.5, 1.0):
        lats = []
        for _ in range(8):
            seq += 1
            r = gm65qr.scan_roundtrip(cdc, cyd, gm65qr.unique_payload(5000 + seq, 10),
                                      deadline_s=25.0, poll_s=poll_s)
            if r["ok"]:
                lats.append(r["latency_s"])
        out["cadences"][str(poll_s)] = {
            "ok": len(lats), "avg_latency_s": round(sum(lats) / len(lats), 2) if lats else None,
        }
    return out


def e4_settings_ab(cdc, cyd):
    """Decode performance under settings 0x81 vs 0x91. The buzzer-armed
    0xD1 leg is deliberately excluded — it beeps on every decode and beep-
    by-default is owner-forbidden; its equivalence is already established
    (2026-09-10 campaign: 8/8 at identical 5.59s, LIMITATIONS.md)."""
    out = {}
    for value, name in ((0x81, "0x81-quiet"), (0x91, "0x91-aim")):
        cdc.drain()
        cdc.send_recv(rig.CMD_SET_SETTINGS, bytes([value]))
        cdc.drain()
        _, verify = cdc.send_recv(rig.CMD_GET_SETTINGS)
        lats, oks = [], 0
        for i in range(8):
            r = gm65qr.scan_roundtrip(cdc, cyd, gm65qr.unique_payload(6000 + i, 10),
                                      deadline_s=15.0)
            if r["ok"]:
                oks += 1
                lats.append(r["latency_s"])
        out[name] = {"readback": verify.hex() if verify else None,
                     "ok": oks, "n": 8,
                     "avg_latency_s": round(sum(lats) / len(lats), 2) if lats else None}
    cdc.drain()
    cdc.send_recv(rig.CMD_SET_SETTINGS, bytes([rig.SETTINGS_SILENT]))
    return out


def pose_canary(cdc):
    """Pose-drift discriminator (#99): one trigger with live ISR counters
    but no delivery points at the POSE, not the module. Called by the
    campaign when E1 lands 0/40."""
    d0 = cdc.diagnostics()
    cdc.trigger()
    time.sleep(1.0)
    status, pl = cdc.read_data()
    d1 = cdc.diagnostics()
    isr0 = d0.get("isr_fires") or 0
    isr1 = d1.get("isr_fires") or 0
    delivered = status == rig.STATUS_OK and len(pl) >= 2
    return {"isr_delta": isr1 - isr0, "delivered_probe": delivered,
            "suspect_pose": (isr1 - isr0) > 0 and not delivered,
            "diag": d1}


def e5_negative_controls(cdc, cyd):
    """Blank screen must yield no NEW decode (after stale consume), and
    scanning must recover after re-rendering."""
    gm65qr.consume_stale(cdc)
    cyd.clear()
    start = time.monotonic()
    fresh = []
    while time.monotonic() - start < 6.0:
        cdc.trigger()
        status, pl = cdc.read_data()
        if status == rig.STATUS_OK and len(pl) >= 2:
            fresh.append(gm65qr.sanitize_scan(pl[1:]).decode(errors="replace"))
        time.sleep(0.4)
    recovery = gm65qr.scan_roundtrip(cdc, cyd, b"recover-after-blank",
                                     deadline_s=15.0)
    return {"blank_window_s": 6.0, "decodes_during_blank": fresh,
            "clean": not fresh, "recovery_ok": recovery["ok"],
            "recovery_latency_s": recovery["latency_s"]}


def e6_wedge_repro(cdc, cyd):
    """Reproduce the settings-write wedge (issue draft 2026-09-10) and prove
    automated recovery. DESTRUCTIVE to the CDC session — caller must
    re-flash/reset afterwards; the campaign runner does. The 0xE9 value has
    the buzzer bit set — the screen is blanked first so no decode (and no
    beep) can occur during the wedge window."""
    observations = {}
    gm65qr.consume_stale(cdc)
    cyd.clear()
    time.sleep(1.0)
    cdc.drain()
    st, _ = cdc.send_recv(rig.CMD_SET_SETTINGS, bytes([0xE9]))
    observations["setsettings_e9_status"] = f"0x{st:02x}" if st is not None else "none"
    time.sleep(1.0)
    cdc.drain()
    st, pl = cdc.send_recv(rig.CMD_STATUS, timeout=3.0)
    observations["status_after"] = f"0x{st:02x}" if st is not None else "none"
    observations["status_payload_len"] = len(pl)
    cdc.drain()
    st, pl = cdc.send_recv(rig.CMD_GET_SETTINGS, timeout=3.0)
    observations["getsettings_after"] = f"0x{st:02x}" if st is not None else "none"
    observations["wedged"] = observations["status_after"] != "0x00"
    # recovery: SWD reset, then wait for the CDC to answer again
    rig.st_reset()
    healed = None
    deadline = time.monotonic() + 60
    import glob as _glob
    while time.monotonic() < deadline:
        port = (_glob.glob("/dev/serial/by-id/usb-gm65-scanner*")
                or _glob.glob("/dev/serial/by-id/*c0de*") or [None])[0]
        if port:
            try:
                probe = rig.StmCdcClient(port, timeout=6)
                probe.drain()
                s, pl2 = probe.send_recv(rig.CMD_STATUS, timeout=4.0)
                if s == rig.STATUS_OK and len(pl2) >= 3:
                    healed = port
                    probe.close()
                    break
                probe.close()
            except Exception:
                pass
        time.sleep(2)
    observations["recovered_via_swd_reset"] = healed is not None
    observations["recovery_port"] = healed
    return observations


def e8_soak_diag(cdc, cyd, n=60):
    """The #92 discriminator: envelope-ladder-style load (varied sizes — the
    profile that reproduces sync degradation at ~cell 8; uniform loops do
    not) with Diagnostic (0x20) polled every 5th scan. The counter that
    spikes as delivery dies names the failing subsystem (isr_ore/uart =>
    UART desync; watchdog/reinit => state machine; all flat => upstream
    drop)."""
    sizes = [7, 10, 14, 24, 34, 44, 58, 72, 92]
    rows = []
    baseline = cdc.diagnostics()
    for i in range(n):
        payload = gm65qr.unique_payload(9000 + i, sizes[i % len(sizes)])
        r = gm65qr.scan_roundtrip(cdc, cyd, payload, deadline_s=15.0)
        row = {"i": i, "size": sizes[i % len(sizes)],
               "ok": r["ok"], "latency_s": r["latency_s"]}
        if i % 5 == 0:
            row["diag"] = cdc.diagnostics()
        rows.append(row)
    final = cdc.diagnostics()
    oks = sum(1 for r in rows if r["ok"])
    return {"baseline": baseline, "final": final, "rows": rows,
            "ok": oks, "n": n,
            "degraded_after": next((r["i"] for r in rows if not r["ok"]), None)}


def e7_jitter_net(cdc, cyd, n=50):
    """Unknown-unknowns catcher: randomized renders (size x cap x payload)
    with continuous polling; every anomaly is recorded, never asserted away."""
    rng = random.Random(20260910)
    anomalies = []
    oks = 0
    for i in range(n):
        size = rng.choice([7, 10, 14, 24, 34, 44, 58, 72])
        cap = rng.choice(CAPS)
        payload = gm65qr.unique_payload(7000 + i, size)
        r = gm65qr.scan_roundtrip(cdc, cyd, payload, cap=cap, deadline_s=10.0)
        if r["ok"]:
            oks += 1
        else:
            anomalies.append({"kind": "timeout", "size": size, "cap": cap,
                              "modules": r["modules"], "px": r["px_per_module"]})
        if r["scanned"] and r["scanned"] != payload:
            anomalies.append({"kind": "mismatch", "size": size, "cap": cap,
                              "scanned": r["scanned"][:40]})
    return {"n": n, "ok": oks, "anomalies": anomalies}
