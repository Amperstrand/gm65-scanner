# Agent Reference

## Hardware

- Board: STM32F469I-Discovery (STM32F469NIHx)
- Scanner: GM65/M3Y, firmware 0x87
- UART: USART6, PG14 (TX) / PG9 (RX), 115200 baud
- USB: USB OTG FS, PA12 (DP) / PA11 (DM)

## Known-Good Pins

| Commit | Notes |
|--------|-------|
| `6744e98` (main HEAD) | Sync 6/6 + CDC 12/12 verified. Async 9/9 HIL verified, CDC enumerates + ScannerStatus 5/5 verified. BSP `ea3b1b2`, embassy BSP `373a9ae`. |

## Production Build Commands

```bash
# Sync firmware (blocking USB stack, screen, scanner)
cargo build --release --target thumbv7em-none-eabihf \
  --manifest-path examples/stm32f469i-disco/Cargo.toml \
  --bin stm32f469i-disco-scanner \
  --no-default-features --features sync-mode

# Async firmware (embassy USB stack, screen, scanner, touch)
cargo build --release --target thumbv7em-none-eabihf \
  --manifest-path examples/stm32f469i-disco/Cargo.toml \
  --bin async_firmware \
  --no-default-features --features scanner-async

# Flash (use st-flash, NOT probe-rs, for USB testing)
arm-none-eabi-objcopy -O binary \
  target/thumbv7em-none-eabihf/release/<binary> /tmp/<binary>.bin
st-flash --connect-under-reset write /tmp/<binary>.bin 0x08000000
st-flash --connect-under-reset reset
```

### Debug builds (with RTT, USB will NOT work)

```bash
# Sync HIL tests (uses probe-rs RTT)
cargo build --release --target thumbv7em-none-eabihf \
  --manifest-path examples/stm32f469i-disco/Cargo.toml \
  --bin hil_test_sync \
  --no-default-features --features hil-tests,defmt

# Async HIL tests (uses probe-rs RTT)
cargo build --release --target thumbv7em-none-eabihf \
  --manifest-path examples/stm32f469i-disco/Cargo.toml \
  --bin hil_test_async \
  --no-default-features --features scanner-async,defmt,gm65-scanner/hil-tests

# Async with RTT debug logging (USB will NOT enumerate)
cargo build --release --target thumbv7em-none-eabihf \
  --manifest-path examples/stm32f469i-disco/Cargo.toml \
  --bin async_firmware \
  --no-default-features --features scanner-async,defmt
```

## HIL Test Results

### 2026-04-05 — Full HIL verification + production firmware test

Both drivers verified with InitAction state machine, BSP `799df39`.

**Sync 6/6 PASS**: init, ping, trigger/stop, timeout, state transitions, QR scan (25 bytes, aim laser)

**Async 9/9 PASS**: init, ping, trigger/stop, timeout, state transitions, cancel+rescan (25 bytes ambient QR), rapid triggers, idle no-trigger, QR scan (23 bytes, aim laser + LED)

Note: BarType VERIFY FAIL observed (wrote 0x01, read 0x05) — expected per known issue #10. cancel_then_rescan picked up ambient QR codes — expected behavior.

### 2026-04-05 — Production Firmware Verification

**Sync**: USB enumerates as `16c0:27dd`, CDC protocol verified (ScannerStatus returns `00 00 03 01 01 01`, GetSettings returns `0x81`). Display + scanner + USB all active. Flash with `st-flash --connect-under-reset`.

**Async**: USB enumerates as `c0de:cafe` but **no data flows** — no heartbeat, no command responses. Firmware runs internally (scanner, display work). See issue #19 for hypotheses and investigation.

**Note**: After this session, two additional fixes were applied: (4) CDC task channel race — `try_receive()` on `CDC_RESPONSE_CHANNEL` polled before scanner task processed the command. Fixed by using `receive().await` after each `COMMAND_CHANNEL.try_send()`. (5) `[ALIVE]` heartbeat every 3s corrupted protocol framing. Fixed by removing heartbeat entirely. Requires on-device verification.

### 2026-03-31 — Full end-to-end with QR scans

Both drivers verified with InitAction state machine, defmt logging parity, BSP `56a0bc8` (HAL 0.5, embedded-hal 1.0).

**Sync 6/6 PASS**: init, ping, trigger/stop, timeout, state transitions, QR scan (18 bytes, aim laser)

**Async 9/9 PASS**: init, ping, trigger/stop, timeout, state transitions, cancel+rescan (24 bytes ambient QR), rapid triggers, idle no-trigger, QR scan (23 bytes, aim laser + LED)

Note: BarType VERIFY FAIL observed (wrote 0x01, read 0x05) — expected per known issue #10. cancel_then_rescan picked up ambient QR codes — expected behavior.

### 2026-03-31 — USB CDC Production Firmware Verification

Both sync and async firmware verified on hardware with `st-flash` (no probe-rs):

**Sync**: USB enumerates as `16c0:27dd`, CDC protocol (ScannerStatus returns `00 00 03 01 01 01`), display boots, scanner init OK, GetSettings returns `0x81`, screen + scanner + USB all active simultaneously.

**Async**: USB enumerates as `c0de:cafe`, embassy CDC with `[ALIVE]` heartbeat, display boots, scanner init OK, screen + scanner + touch + USB all active simultaneously.

### 2026-03-28 — Native binaries, both drivers, real QR scans

**Async 9/9 PASS**: init, ping, trigger/stop, timeout, state transitions, cancel+rescan, rapid triggers, idle no-trigger, QR scan (25 bytes, aim laser + LED)

**Sync 6/6 PASS**: init, ping, trigger/stop, timeout, state transitions, QR scan (aim laser, 50-retry loop)

## Known Issues

- **BarType register not persisted (#10)**: Register 0x002C write accepted but not persisted across GM65 reboots on firmware 0.87. Hardware quirk.
- **Settings mode comparison (#11)**: 0x81 vs 0xD1 not yet compared. Current firmware uses 0x81.
- **drain_uart data loss (#12)**: FIXED. `send_command()` skips drain when in `Scanning` state.
- **Async CDC no data flow (#19)**: RESOLVED. Five root causes found and fixed: (1) PLLSAI `divq: None` crashes MCU, (2) double `USART6.disable()` crashes MCU, (3) `AsyncUart::read()` busy-poll starves USB in cooperative executor, (4) CDC task channel race — `try_receive()` on `CDC_RESPONSE_CHANNEL` polled before scanner task processed command, fixed by using `receive().await` after each command send, (5) `[ALIVE]` heartbeat every 3s corrupted protocol framing, fixed by removing heartbeat entirely. Additional fix: mutex guards held across `.await` causing deadlocks. Async firmware now enumerates and responds to CDC commands. See issue for full details.
- **BSP memory.x wrong flash size**: embassy-stm32f469i-disco `memory.x` declares 1024K flash but STM32F469NIHx has 2048K. Filed as [Amperstrand/embassy-stm32f469i-disco#19](https://github.com/Amperstrand/embassy-stm32f469i-disco/issues/19).
- **Heap/framebuffer overlap**: `DisplayOrientation::fb_size()` returns pixels (384,000), not bytes. Framebuffer uses `u16` (2 bytes/pixel), so actual size is `fb_size() * 2` (768,000 bytes). Heap offset must account for this or allocator metadata gets corrupted by display writes.

## USB CDC + defmt_rtt Incompatibility (RESOLVED)

`defmt_rtt` (even when unused via `use defmt_rtt as _`) prevents USB OTG FS enumeration. See [stm32f469i-disc#23](https://github.com/Amperstrand/stm32f469i-disc/issues/23).

### Root cause analysis

Two interacting problems:

1. **`defmt_rtt` uses `critical_section::acquire()`** for every log write (defmt-rs/defmt `lib.rs:224`), which disables **all interrupts** including USB OTG. STM32F4 OTG FS requires precise interrupt timing during enumeration (RM0090 §32.4.4, see also [embassy-rs/embassy#2823](https://github.com/embassy-rs/embassy/pull/2823)). Early logging during USB init enters critical sections at the wrong time → host times out.

2. **probe-rs hardcodes blocking mode** for the "defmt" RTT channel ([probe-rs `client.rs:282-284`](https://github.com/probe-rs/probe-rs/blob/7885394/probe-rs-tools/src/bin/probe-rs/util/rtt/client.rs#L282-L284)). If any log occurs during enumeration and the RTT buffer fills, `flush()` busy-waits with interrupts disabled → USB stalls indefinitely. See [knurling-rs/defmt#133](https://github.com/knurling-rs/defmt/issues/133).

This is **not a defmt bug** — it's a fundamental conflict between RTT's interrupt-disabling design and USB OTG's timing requirements. See also [embassy-rs/embassy#3493](https://github.com/embassy-rs/embassy/issues/3493) (defmt-trace timing affects USB behavior) and [embassy-rs/embassy#4008](https://github.com/embassy-rs/embassy/issues/4008) (users requesting defmt-free builds).

### Ecosystem context

This BSP's unconditional `"defmt"` in HAL features was **non-standard**. Every major STM32 HAL makes defmt optional: [stm32f4xx-hal](https://github.com/stm32-rs/stm32f4xx-hal/blob/v0.23.0/Cargo.toml#L30-L31), [stm32f1xx-hal](https://github.com/stm32-rs/stm32f1xx-hal/blob/v0.11.0/Cargo.toml#L34-L39), [stm32h7xx-hal](https://github.com/stm32-rs/stm32h7xx-hal/blob/v0.16.0/Cargo.toml#L43-L46), [stm32f3xx-hal](https://github.com/stm32-rs/stm32f3xx-hal/blob/v0.10.0/Cargo.toml#L33-L34), [rp2040-hal](https://github.com/rp-rs/rp-hal/blob/main/rp2040-hal/Cargo.toml#L39-L40), and [embassy-stm32](https://github.com/embassy-rs/embassy/blob/embassy-stm32-v0.6.0/embassy-stm32/Cargo.toml#L60-L71) all use `defmt = { optional = true }` with an opt-in feature flag. The official stm32f429i-disc BSP uses `default-features = false` on its HAL dep ([Cargo.toml](https://github.com/stm32-rs/stm32f429i-disc/blob/v0.3.0/Cargo.toml#L25-L27)).

**Do NOT use defmt_rtt or panic_probe in firmware that enables USB CDC.**

### Fix (applied in this repo)

1. **Workspace deps**: `default-features = false` on `stm32f469i-disc` and `gm65-scanner` workspace dependencies so `defmt`/`defmt-rtt` don't leak into production builds.
2. **Embassy defmt decoupling**: Removed `defmt` feature from workspace `embassy-time`, `embassy-executor`, `embassy-stm32` pins. Only enabled when this crate's explicit `defmt` feature is on.
3. **Conditional panic handlers**: Production builds use `panic_halt`; `panic_probe` only with `defmt` feature.
4. **Conditional `defmt.x`**: `build.rs` generates an empty `defmt.x` in OUT_DIR when defmt is OFF (satisfying `-Tdefmt.x` in `.cargo/config.toml`). When defmt IS ON, the build.rs skips generation so the `defmt` crate's real `defmt.x` (with `_defmt_timestamp` PROVIDE) is found via its own `cargo:rustc-link-search`.
5. **Feature structure**: `sync-mode` does NOT include `defmt-rtt`. `scanner-async` does NOT include `hil-tests` or `defmt`. `defmt` feature enables RTT + probe for debug/HIL builds only.

## Embassy Async: USB + Scanner Init Ordering (RESOLVED)

**Bug**: In embassy's cooperative executor, `scanner.init().await` was called in `main()` BEFORE the `join4` that starts `usb_dev.run()`. The executor couldn't poll USB during the blocking scanner UART init, so USB never enumerated.

**Fix**: Move `Gm65ScannerAsync::with_default_config()` + `scanner.init().await` into the scanner task itself, so USB polling starts concurrently via `join4`.

**Also required**: Disable USART6 interrupt before creating the UART (`embassy_stm32::interrupt::USART6.disable()`), and register a USART6 handler in `bind_interrupts!` to catch spurious interrupts.

## Async CDC: Three Root Causes (RESOLVED)

**Bug**: Async production firmware enumerated as `c0de:cafe` but no data flowed over USB. Three independent root causes found:

1. **PLLSAI `divq: None` crashes MCU**: `config.rcc.pllsai` with `divq: None` causes immediate hard fault after USB enumeration. Fixed by setting `divq: Some(PllQDiv::DIV8)`, matching BSP commit `c136f11`.

2. **Double `USART6.disable()` crashes MCU**: Two consecutive `embassy_stm32::interrupt::USART6.disable()` calls after UART creation — second call triggers undefined behavior. Fixed by removing the duplicate.

3. **`AsyncUart::read()` busy-poll starves USB**: `yield_threshold = 500_000` causes `read()` to spin up to 500K times without yielding, completely starving `usb_dev.run()` in embassy's cooperative executor. Fixed by yielding immediately on every `WouldBlock`.

**Additional**: Mutex guards held across `.await` (e.g., `SHARED.lock().await` held during `scanner.get_scanner_settings().await`) caused deadlocks. Fixed by splitting lock scopes.

## Async CDC: Remaining Issues

- **Scanner task blocks on auto_scan**: During `read_scan()` (up to 10s timeout), `COMMAND_CHANNEL.try_receive()` is not polled. CDC commands sent during auto_scan are queued but not processed until the scan cycle completes. Fix: use `embassy_futures::select` to handle commands while scanning.
- **GetSettings/Trigger fail during auto_scan**: Same root cause as above — scanner task can't process CDC commands while awaiting scan result.
- **PLLSAI1_Q breaks USB enumeration**: BSP commit `c136f11` uses `PLLSAI1_Q` for 48MHz but this doesn't enumerate on our hardware. `PLL1_Q` works (same 48MHz, different PLL source). Likely PLLSAI startup timing issue.
- Touch controller uses I2C2/PB10/PB11 but BSP doc says I2C1/PB8/PB9

## Upstream Interaction Policy

**NEVER file PRs or issues on upstream projects without human review.** See [Amperstrand/micronuts#19](https://github.com/Amperstrand/micronuts/issues/19) for retrospective.

## ST-LINK Recovery

Kill stale processes before any probe-rs operation:
```
pkill -9 probe-rs; sleep 3
```

If `interface is busy` persists or xHCI controller dies (all USB devices vanish):
```
echo 1 | sudo tee /sys/bus/pci/devices/0000:02:00.0/remove
sleep 2
echo 1 | sudo tee /sys/bus/pci/rescan
sleep 3
probe-rs list
```

Find PCI address on other machines: `sudo lspci -nn | grep -i "xHCI"`

Use `st-flash --connect-under-reset` for CDC testing. `probe-rs` holds SWD and prevents the firmware from running — use probe-rs only for RTT-based HIL tests.
