# Agent Reference

## Hardware

- Board: STM32F469I-Discovery (STM32F469NIHx)
- Scanner: GM65/M3Y, firmware 0x87
- UART: USART6, PG14 (TX) / PG9 (RX), 115200 baud
- USB: USB OTG FS, PA12 (DP) / PA11 (DM)

## Known-Good Pins

| Commit | Notes |
|--------|-------|
| Pending (main HEAD) | Sync USB+Display+Scanner CDC verified. Async USB+Display+Scanner+Touch CDC verified. BSP `799df39`, embassy BSP `e202e9a`. |
| `1360469` | Sync 6/6, async 9/9, QR scans on both. BSP `56a0bc8`, embassy BSP `890a4d1`. |

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
- **Heap/framebuffer overlap**: `DisplayOrientation::fb_size()` returns pixels (384,000), not bytes. Framebuffer uses `u16` (2 bytes/pixel), so actual size is `fb_size() * 2` (768,000 bytes). Heap offset must account for this or allocator metadata gets corrupted by display writes.

## USB CDC + defmt_rtt Incompatibility (RESOLVED)

`defmt_rtt` (even when unused via `use defmt_rtt as _`) prevents USB OTG FS enumeration. Root cause unknown — possibly SWD/ITM contention. **Do NOT use defmt_rtt or panic_probe in firmware that enables USB CDC.**

### Fix (applied in this repo)

1. **Workspace deps**: `default-features = false` on `stm32f469i-disc` and `gm65-scanner` workspace dependencies so `defmt`/`defmt-rtt` don't leak into production builds.
2. **Embassy defmt decoupling**: Removed `defmt` feature from workspace `embassy-time`, `embassy-executor`, `embassy-stm32` pins. Only enabled when this crate's explicit `defmt` feature is on.
3. **Conditional panic handlers**: Production builds use `panic_halt`; `panic_probe` only with `defmt` feature.
4. **Fallback `defmt.x`**: Empty linker script at repo root so non-defmt builds link with existing `.cargo/config.toml`.
5. **Feature structure**: `sync-mode` does NOT include `defmt-rtt`. `scanner-async` does NOT include `hil-tests` or `defmt`. `defmt` feature enables RTT + probe for debug/HIL builds only.

## Embassy Async: USB + Scanner Init Ordering (RESOLVED)

**Bug**: In embassy's cooperative executor, `scanner.init().await` was called in `main()` BEFORE the `join4` that starts `usb_dev.run()`. The executor couldn't poll USB during the blocking scanner UART init, so USB never enumerated.

**Fix**: Move `Gm65ScannerAsync::with_default_config()` + `scanner.init().await` into the scanner task itself, so USB polling starts concurrently via `join4`.

**Also required**: Disable USART6 interrupt before creating the UART (`embassy_stm32::interrupt::USART6.disable()`), and register a USART6 handler in `bind_interrupts!` to catch spurious interrupts. Reduce `AsyncUart.yield_threshold` from 2M to 500K.

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

Use `probe-rs download` + `probe-rs reset` (not `probe-rs run`) for CDC testing so probe-rs releases the ST-LINK.
