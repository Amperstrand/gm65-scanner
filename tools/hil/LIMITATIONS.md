# QR Rig Limitations — measured characterization (campaign 2026-09-10)

Reference for any Amperstrand project driving the CYD→GM65 loopback rig
(`tools/hil/gm65qr.py` + `tools/hil/rig.py`). Full data:
`tools/hil/results/campaign-20260910-105412/` (+ `e2rerun-*`).

## Reliability (E1: 40 unique-payload roundtrips, winning config)

| Firmware | Success | Latency profile |
|----------|---------|-----------------|
| async (HEAD) | **40/40 (100%)** | poll-instant p50, max 2.6s |
| sync (HEAD) | **40/40 (100%)** | consistent ~5.6s (auto-scan re-arm cadence) |

## Scan speed (E3)

- Sustained unique-payload throughput (async, healthy): **7.1 decodes/min**
  (8.5s/scan cycle including render + poll overhead).
- Host poll cadence (0.2/0.5/1.0s) does not change decode latency — the
  firmware delivers by the next poll once the module decodes.
- The 5.6s sync steady-state is the firmware's auto-scan re-arm cycle, not
  module decode time.

## Settings A/B (E4 — resolves repo issue #11)

Under healthy conditions, **0x81, 0x91, and 0xD1 decode identically**
(sync: 8/8 at the same 5.59s avg for each; readbacks verified). Choose by
UX: 0x81 fully quiet, 0x91 adds the aim LED while reading, 0xD1 adds the
decode buzzer. No performance reason to prefer any.

## QR envelope on the CYD (E2 rerun, 2026-09-10)

- CYD firmware line-protocol cap: payloads ≤ ~240 bytes (249+ byte hex
  lines overflow LINE_MAX by one character — off-by-one, bench-measured).
- **Measured decode envelope (async, healthy, ECC-H): a sharp cliff at 53
  modules.** Everything ≤53 modules decodes at ALL caps (160/192/224) and
  even at 3px/module; ≥57 modules fails at every cap including 3px/module.
  2px/module is always below the floor.

| Payload (ECC-H) | Modules | Result |
|---|---|---|
| 7–58 B | 21–41 | decodes everywhere (3–7 px/module) |
| 72–92 B | 49–53 | decodes at ≥3px/module (fails at 2px) |
| **116–240 B** | 57–81 | **never decodes** — module-count ceiling |

- **Practical limit for consumers: 92 bytes per QR frame** at the winning
  config. Longer content must use UR multi-part framing (the crate's
  `UrDecoder` already handles reassembly).
- Sync carried only the first cells before the sustained-load degradation
  collapsed it (see below) — the envelope above is the async reference;
  sync requires interleaved SetSettings resyncs for long campaigns.

## Known degradation modes (unknown-unknowns surfaced — E3→E4 transition, E5, E7)

1. **Sustained-load scan-delivery degradation (both firmwares).** After
   ~10 min of back-to-back envelope renders/decodes, scan delivery
   collapses (async: after 30 sustained decodes + SetSettings; sync:
   earlier — cell ~8 of the envelope ladder). CDC stays healthy;
   ScannerStatus still reports connected=1. Recovery differs:
   - async: SWD reset (reboot) restores (E7 post-reset: 38/50).
   - sync: a SetSettings command heals it in place (its stop_scan +
     readback sequence resyncs the UART path) — E4 went 8/8×3 immediately.
   Likely firmware state-machine/UART desync under load — issue-worthy.
2. **Sync self-healing × blank screen (E5).** A blank screen longer than
   ~6s trips the watchdog 3× → the firmware's self-healing enters
   continuous mode → the CDC virtual-human trigger/poll loop no longer
   drives scans until a SetSettings-class resync. Negative windows on
   sync must stay short or issue a SetSettings afterwards.
3. **CYD payload cap** (above): ≤240B.
4. **0xE9 settings value no longer wedges HEAD** (E6, both firmwares:
   accepted, protocol stayed healthy) — the wedge in the issue draft is
   April-era (74686b6) behavior; the SetSettings result-discarding code
   smell remains (async handler ignores the write bool).

## Rig recovery playbook (built into rig.py)

- st-flash write → always explicit `--connect-under-reset reset`
- CDC absent after flash → one-shot xHCI bounce (PCI 0000:07:00.3
  remove/rescan) inside every wait
- scanner task settle: retry ScannerStatus up to ~30s post-boot
- identify CDCs by /dev/serial/by-id product string (three known:
  `QR Barcode Scanner` sync, `QR Scanner f469disco` async, `Cashu
  Hardware Wallet` micronuts — all may share 16c0:27dd/F4691)
