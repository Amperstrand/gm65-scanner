# Issue draft: rejected SetSettings value wedges CDC protocol until board reset

> Owner-gated: ready to paste into GitHub. Filed as a draft here per the
> external-posting policy (agents never post; owner copy-pastes).
>
> **UPDATE 2026-09-10 (campaign E6)**: NOT reproducible on HEAD (both
> firmwares accepted 0xE9 with a healthy protocol afterwards). The wedge
> was April-era (`74686b6`) behavior. The result-discarding code smell
> below remains valid, and the campaign surfaced a RELATED live bug: see
> "sustained-load scan-delivery degradation" in tools/hil/LIMITATIONS.md.

- **Title**: Rejected SetSettings value permanently desyncs the CDC
  protocol until power cycle / board reset
- **Severity**: Medium (bench/host tooling reliability; recovery = reset)
- **Component**: `examples/stm32f469i-disco/src/bin/async_firmware.rs` (and
  the same pattern likely in `src/main.rs` sync firmware)
- **Observed on**: async firmware `74686b6` (2026-09-08 bench session); code
  path unchanged at HEAD (`9db7907`+).

## Reproduction

1. Flash async firmware; wait for `c0de:cafe` CDC + scanner ready
   (ScannerStatus → `00 00 03 01 01 01`).
2. Send `SetSettings` with a value the GM65 module rejects: `14 00 01 E9`.
   (0xE9 = aim Always + light Always — rejected by this module, fw 0x87;
   0x81/0x91/0x95/0xC1/0xD1/0xD5 are all accepted.)
3. Response arrives with status `0xFF` (Error) and empty readback.

## Observed failure (byte-level, bench log 2026-09-08)

After the rejected write, every subsequent CDC command returns wrong or
malformed responses — the response/state channel is desynced:

| Command (host → fw)     | Response          | Expected |
|-------------------------|-------------------|----------|
| `SetSettings 14 00 01 E9` | `FF …` (Error)    | Error status is fine, but see below |
| `ScannerStatus 10 00 00`  | `FF …` empty      | `00 00 03 01 01 01` |
| `GetSettings 13 00 00`    | `00` + **empty payload** | `00 00 01 <bits>` |
| `SetSettings 14 00 01 E1` | `12 …`            | `00 …` readback |
| `GetSettings 13 00 00`    | `12 …`            | `00 00 01 <bits>` |

`st-flash --connect-under-reset reset` restores full operation
(ScannerStatus connected=1, settings readback intact).

## Root cause (code inspection)

`examples/stm32f469i-disco/src/bin/async_firmware.rs`, `HostCommand::SetSettings(s)`
handler (~line 542):

```rust
scanner.cancel_scan();
let _ = scanner.stop_scan().await;
scanner.set_scanner_settings(s).await;          // <-- bool result DISCARDED
Timer::after_millis(SETTINGS_COMMIT_DELAY_MS).await;
if let Some(readback) = scanner.get_scanner_settings().await { ... }
```

- `set_scanner_settings()` (`crates/gm65-scanner/src/driver/async_.rs:93`)
  returns `bool` (success of the register write); the firmware ignores it.
  The NVRAM persist path also discards: `let _ = self.save_settings().await;`
  (`async_.rs:243`).
- When the module rejects the value, the failed transaction leaves stale
  and/or partial response bytes in the scanner UART buffer; nothing drains
  or resyncs it on the error path. Every later `send_command()` parses a
  shifted/garbage byte stream — explaining the mixed `FF`/empty/`12`
  responses until a hard reset.
- The readback branch masks the failure further: it reports
  `SetSettingsWriteFailed` only when the READ fails, not when the WRITE
  failed.

## Suggested fix

1. Check the `set_scanner_settings()` result; on `false`, respond
   `Status::Error` immediately with a distinct payload (host can tell
   "module rejected value" from "readback failed").
2. On any failed scanner transaction, drain/resync the UART receive buffer
   (reuse the `drain_uart` logic, respecting the `Scanning`-state exception
   from issue #12) before the next command, so one bad register value can
   never wedge the protocol.
3. Surface `save_settings()` failures in the response (currently silently
   ignored — settings take effect but do not persist; the host cannot know).

## Fix-verification sketch

- Host-side regression in `tools/hil`: send `14 00 01 E9`, then assert
  `ScannerStatus` and `GetSettings` still answer correctly (no reset).
  Requires the flash-mutation harness profile; add next to
  `test_qr_loopback.py`.
