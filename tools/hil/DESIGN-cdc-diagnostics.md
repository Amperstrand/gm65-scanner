# Design: CDC Diagnostic command (0x20)

> Updated 2026-09-10 (late): the wire already exists — the sync firmware
> ships a rich `Diagnostic = 0x20` handler (interrupt-ring-buffer era).
> This doc describes the SHIPPED layout and the remaining async gap.
> Original 0x17 draft superseded.

## Purpose

The sustained-load scan-delivery degradation (#92) is characterized but not
root-caused. Diagnostic (0x20) exposes the counters the firmware tracks;
the counter that spikes as delivery dies names the failing subsystem:
- `isr_overrun_errors` / `uart_errors` spike -> UART desync under load
- `watchdog_count` / `reinit_count` spike -> state-machine wedging
- all flat while delivery dies -> upstream (channel/state) drop

## Sync firmware layout (16 bytes, little-endian, shipped)

| Bytes | Field |
|---|---|
| 0-1 | scan_count (u16) |
| 2-3 | nak_count (u16) |
| 4 | watchdog_count (u8, truncated) |
| 5 | reinit_count (u8, truncated) |
| 6 | scanner state (1=Ready 2=Scanning 3=ScanComplete 4=Error 0=other) |
| 7 | live settings byte |
| 8 | ring buffer length |
| 9-11 | isr_bytes (u24) — bytes seen by the UART ISR |
| 12-13 | isr_overrun_errors (u16) — the UART-desync signal |
| 14-15 | isr_fires (u16) |

## Async firmware layout (10 bytes, little-endian — TODO #91 remainder)

| Bytes | Field |
|---|---|
| 0-3 | scans_delivered (u32) |
| 4-7 | uart_errors (u32, from AsyncUart::uart_error_count) |
| 8 | scanner state |
| 9 | auto_scan flag |

Client: rig.StmCdcClient.diagnostics() auto-detects the layout.
Experiment: experiments.e8_soak_diag — scan loop, 0x20 polled every 5th
scan; the transition row is the evidence #92 needs.
