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
    collapses. CDC stays healthy; ScannerStatus still reports connected=1.
    Reproduced on healthy boots (twice on sync, once on async; 2026-09-10):
    - async: collapses after ~30 sustained decodes; SWD reset restores
      (post-reset jitter 38/50).
    - sync: collapses at ~cell 8 of the envelope ladder (9/45 cells
      pass — only the first sizes decode), then E3/E4/E7 stay at ~0 even
      after E6's SWD reset (1/50) and despite SetSettings writes. The
      earlier "SetSettings heals in place" observation did NOT reproduce —
      time-at-rest is a confound. Only a fresh flash+boot reliably recovers
      sync (E1 40/40 after every bringup).
    Likely firmware state-machine/UART desync under load — needs the
    DIAGNOSTICS counters (#91/#92) to discriminate.

   **2026-09-11 discriminator update (soak_diag, pose in-pocket):** the
   transition is FIRMWARE-side staging, not module death and not UART
   overrun — isr_overrun_errors stayed 0 through collapse; scan_count
   climbed to 143 while only 16/60 delivered; ring_len flips 0↔14 (bytes
   arrive and sit unstaged); losses are size-biased (≤14B mostly deliver,
   ≥24B mostly lost); deliveries that do land cluster on the 5s retrigger
   boundary (a stop/start cycle flushes the stuck path). A fresh F469
   boot delivers instantly — including the payloads that "failed" during
   collapse (the module had decoded and buffered them). The async variant
   is a carryover choke: after sustained sync load, fresh async boots
   gate-fail with uart_errors=19 until 0x22 heals from a fresh-boot state.
   ur_e2e first run on sync: 1/3 single, 0/3 sequence (stall); the UR
   transport itself is not the blocker.

2. **Sync self-healing × blank screen (E5).** A blank screen longer than
   ~6s trips the watchdog 3× → the firmware's self-healing enters
   continuous mode → the CDC virtual-human trigger/poll loop no longer
   drives scans until a SetSettings-class resync. Negative windows on
   sync must stay short or issue a SetSettings afterwards.

   **2026-09-11 update (commit daf833a):** the continuous-mode defect is
   fixed — self-heal now records `continuous_active` (the flag was never
   set, so the gates never engaged) and any CDC frame exits continuous
   mode (`exit_continuous_mode`), mirroring the virtual-human dismissal.
   E1 soaks 40/40 with the fix. E5's `recovery_ok=false` persists — but
   the instrumented repro shows the 6s blank trips only ONE watchdog
   (self-heal never engages) and recovery passes on a fresh boot; the
   campaign-E5 failure is #92's sustained-load staging race inherited
   from E1's tail, not the continuous-mode path. E5 acceptance is owned
   by #92.

3. **CYD payload cap** (above): ≤240B.
4. **0xE9 settings value no longer wedges HEAD** (E6, both firmwares:
   accepted, protocol stayed healthy) — the wedge in the issue draft is
   April-era (74686b6) behavior; the SetSettings result-discarding code
   smell remains (async handler ignores the write bool).
5. **Module heal tiers (2026-09-11 validation).** 0xA5 (0x23): module
   accepts the deep-sleep reboot but did not re-answer ping and healed
   nothing — Tier A negative (details:
   docs/issue-drafts/2026-09-11-module-reboot-tier-a-negative.md). 0x22
   (FactoryReset + baud dance): heals the carryover choke, but from a
   desynced state the first attempt can fail (reinit_ok=0, module left at
   model=0) — a second 0x22 from the fresh-boot firmware state healed it
   reliably ([accepted=0, reinit_ok=1, model=1]). Treat 0x22 as
   retry-once-from-fresh-boot.

## Rig recovery playbook (built into rig.py)

- st-flash write → always explicit `--connect-under-reset reset`
- CDC absent after flash → one-shot xHCI bounce (PCI 0000:07:00.3
  remove/rescan) inside every wait
- scanner task settle: retry ScannerStatus up to ~30s post-boot
- identify CDCs by /dev/serial/by-id product string (three known:
  `QR Barcode Scanner` sync, `QR Scanner f469disco` async, `Cashu
  Hardware Wallet` micronuts — all may share 16c0:27dd/F4691)
