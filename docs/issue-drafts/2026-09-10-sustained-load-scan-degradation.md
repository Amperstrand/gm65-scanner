# Issue draft: sustained-load scan-delivery degradation (both firmwares)

> Owner-gated: ready to paste into GitHub.

- **Title**: Scan delivery collapses after sustained scanning; CDC reports
  healthy — recovery differs per firmware
- **Severity**: High for long-running hosts (wallets, soaks); single scans
  unaffected
- **Evidence**: characterization campaign 2026-09-10
  (`tools/hil/results/campaign-20260910-105412/` + `e2rerun-*`),
  summarized in `tools/hil/LIMITATIONS.md` §1. Reproduced twice
  (campaign + envelope rerun).

## Observed

| Firmware | Trigger | Symptom | Recovery |
|---|---|---|---|
| async | ~30 sustained decodes then SetSettings | 0-1/8 decodes (13s deadline), ScannerStatus healthy | SWD reset (E7 post-reset: 38/50) |
| sync | ~8 envelope cells back-to-back | 0/30 sustained decodes; E5 recovery-after-blank fails | SetSettings heals in place (E4: 8/8 ×3 immediately) |

Sync additionally stalls when the blank-screen watchdog self-healing
enters continuous mode (E5) — the CDC virtual-human loop
(`d0b8d3b` fix) does not exit continuous mode.

## Hypotheses (see docs/DESIGN-cdc-diagnostics.md for the discriminator)

1. UART desync under load (stale response bytes starve the read path) —
   sync's SetSettings heals via its stop_scan+readback resync sequence,
   which supports this.
2. Watchdog/self-healing state machine wedging under continuous load.
3. Scan channel/state drop with counters flat (needs DIAGNOSTICS 0x17 to
   distinguish).

## Suggested path

1. Land CDC DIAGNOSTICS (design doc) — counters exist in both firmwares.
2. `soak_diag` experiment: poll counters alongside scans until death; the
   spiking counter names the subsystem.
3. Fix per subsystem; the campaign becomes the regression gate.

## Workarounds (documented in LIMITATIONS.md)

- Interleave a SetSettings write every N scans on sync (heals in place).
- Periodic SWD reset on async for unattended long runs.
