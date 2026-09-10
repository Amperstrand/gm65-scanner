# Design: CDC DIAGNOSTICS command (0x17)

> Drafted 2026-09-10 during the characterization campaign. The sustained-
> load scan-delivery degradation (tools/hil/LIMITATIONS.md §1) is
> characterized but not root-caused — this command makes it observable and
> bisectable. Both firmwares ALREADY track the counters internally; this
> only exposes them.

## Command

`DIAGNOSTICS = 0x17`, 3-byte frame `17 00 00`.

Response: `00 <len_hi> <len_lo>` + 5 big-endian u32 counters (20 bytes):

| Offset | Counter | Source (existing) |
|--------|---------|-------------------|
| 0 | uart_errors | async: `AsyncUart::uart_error_count` (#54); sync: new counter on nb::Error::Other |
| 4 | scan_count | sync: `diag.scan_count`; async: count in ScannerDataCdc take()-Some path |
| 8 | watchdog_count | sync: `diag.watchdog_count`; async: new (scan timeouts) |
| 12 | reinit_count | sync: `diag.reinit_count`; async: init retries |
| 16 | nak_count | sync: `diag.nak_count`; async: new |

## Why this is the keystone

The degradation kills scan delivery while CDC stays healthy and counters
exist but are invisible. With DIAGNOSTICS, the soak experiment polls
counters alongside scans; the counter that spikes at delivery-death names
the failing subsystem:

- uart_errors spikes → UART desync under load → fix = drain/resync policy
- watchdog/reinit spike → watchdog starving under load → timing fix
- all flat while delivery dies → scan data dropped upstream (channel/state)

## Experiment it unlocks (soak_diag)

E1-style loop; every 5th scan also polls DIAGNOSTICS; artifacts log
(scan_i, latency, counters-delta). Run until degradation; the transition
row is the evidence an issue needs.

## Implementation notes

- cdc.rs: extend `Command` enum + FrameDecoder mapping (0x17 => Some).
- Both firmwares: serialize counters into the response payload.
- rig.py: `StmCdcClient.diagnostics() -> dict`.
- Backward compat: old hosts never send 0x17; new hosts handle missing
  response (timeout) gracefully.
