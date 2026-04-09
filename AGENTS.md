# Agent Reference

## Hardware

- Board: STM32F469I-Discovery (STM32F469NIHx)
- Scanner: GM65/M3Y, firmware 0x87
- UART: USART6, PG14 (TX) / PG9 (RX), 115200 baud
- USB: USB OTG FS, PA12 (DP) / PA11 (DM)
- Display: 480×800 portrait via DSI/LTDC (NT35510), RGB888/ARGB8888 pixel format
- SDRAM: 16MB via FMC, framebuffer 1.5MB (u32 × 384000 pixels)
- Touch: FT6X06 on I2C1 (PB8=SCL, PB9=SDA), identity transform
- Clock: 180MHz SYSCLK, PLLSAI_Q=48MHz for USB, PLLSAI_R for LTDC pixel clock

## Known-Good Pins

| Commit | Notes |
|--------|-------|
| `83ecbad` (main HEAD) | Sync 6/6 + CDC 12/12 verified. Async 9/9 HIL verified, CDC enumerates + ScannerStatus 5/5 verified. Touch calibration verified (identity transform, portrait 480x800). touch_test binary HW verified. BSP `ea3b1b2`, embassy BSP `373a9ae`. |
| `UNCOMMITTED` | **Async display + DisplayCtrl WORKS.** `display_minimal` and `display_hybrid` both HW verified. `display_hybrid` uses BSP fork's `DisplayCtrl::new()` API with hardcoded NT35510 init, stm32_metapac typed LTDC accessors, RGB888/ARGB8888, portrait 480x800 at 180MHz. `async_firmware` updated to 180MHz clock (PLL1 DIV8/MUL360, PLLSAI_Q for USB 48MHz). All render code migrated from Rgb565 to Rgb888. See Async Display Resolved section below. |
| `UNCOMMITTED` | **defmt leak fix in BSP dep.** `embassy-stm32f469i-disco` dependency changed from `features = ["defmt"]` (unconditional) to `default-features = false, features = ["display", "touch"]` + conditional `embassy-stm32f469i-disco/defmt` via workspace `defmt` feature. Both `scanner-async` (no defmt, USB works) and `scanner-async,defmt` (RTT logging, no USB) builds pass. |

## Touch Calibration

- **Touch controller**: FT6X06 on I2C1 (PB8=SCL, PB9=SDA)
- **Vendor ID**: 0x11 (verified)
- **Coordinate transform**: Identity — `dx=tx, dy=ty` (raw FT6X06 coords map directly to display pixels)
- **Framebuffer**: Portrait 480x800 (display orientation is Portrait, NOT Landscape)
- **Key finding**: FT6X06 X register (0x03-0x04) ranges 0-480, Y register (0x05-0x06) ranges 0-800
- **Touch test binary**: `touch_test` — 6 target rectangles, raw coordinate display, hit detection. HW verified.
- **BSP issue**: embassy-stm32f469i-disco#21 documents the missing orientation-dependent transform

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

# Touch calibration test (display + touch, no scanner/USB)
cargo build --release --target thumbv7em-none-eabihf \
  --manifest-path examples/stm32f469i-disco/Cargo.toml \
  --bin touch_test \
  --no-default-features --features sync-mode
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
- **Heap/framebuffer overlap**: `DisplayOrientation::fb_size()` returns pixels (384,000), not bytes. Framebuffer uses `u32` (4 bytes/pixel, ARGB8888), so actual size is `fb_size() * 4` (1,536,000 bytes). Heap offset must account for this or allocator metadata gets corrupted by display writes. Previously was `u16` (Rgb565) at 2 bytes/pixel.

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
6. **BSP fork defmt leak** (2026-04-09): `embassy-stm32f469i-disco` workspace dependency changed from `features = ["defmt"]` (unconditional) to `default-features = false, features = ["display", "touch"]` + conditional `embassy-stm32f469i-disco/defmt` via workspace `defmt` feature. This prevents defmt symbols from leaking into production (USB CDC) builds.

## Embassy Async: USB + Scanner Init Ordering (RESOLVED)

**Bug**: In embassy's cooperative executor, `scanner.init().await` was called in `main()` BEFORE the `join4` that starts `usb_dev.run()`. The executor couldn't poll USB during the blocking scanner UART init, so USB never enumerated.

**Fix**: Move `Gm65ScannerAsync::with_default_config()` + `scanner.init().await` into the scanner task itself, so USB polling starts concurrently via `join4`.

**Also required**: Disable USART6 interrupt before creating the UART (`embassy_stm32::interrupt::USART6.disable()`), and register a USART6 handler in `bind_interrupts!` to catch spurious interrupts.

## Async CDC: Three Root Causes (RESOLVED)

**Bug**: Async production firmware enumerated as `c0de:cafe` but no data flowed over USB. Three independent root causes found:

1. **PLLSAI `divq: None` crashes MCU**: `config.rcc.pllsai` with `divq: None` causes immediate hard fault after USB enumeration. Fixed by setting `divq: Some(PllQDiv::DIV8)`, matching BSP commit `c136f11`.

   **IMPORTANT UPDATE (2026-04-09)**: `divq: None` does NOT crash MCU — this was a misdiagnosis. The display works with `divq: None`. The real crash was caused by other issues (double USART6.disable, AsyncUart yield starvation). The display requires `divq: None` + PLLSAI_R for pixel clock. USB 48MHz comes from PLL1_Q (not PLLSAI_Q). These are separate clock paths.

2. **Double `USART6.disable()` crashes MCU**: Two consecutive `embassy_stm32::interrupt::USART6.disable()` calls after UART creation — second call triggers undefined behavior. Fixed by removing the duplicate.

3. **`AsyncUart::read()` busy-poll starves USB**: `yield_threshold = 500_000` causes `read()` to spin up to 500K times without yielding, completely starving `usb_dev.run()` in embassy's cooperative executor. Fixed by yielding immediately on every `WouldBlock`.

**Additional**: Mutex guards held across `.await` (e.g., `SHARED.lock().await` held during `scanner.get_scanner_settings().await`) caused deadlocks. Fixed by splitting lock scopes.

## Async CDC: Remaining Issues

- **Scanner task blocks on auto_scan**: During `read_scan()` (up to 10s timeout), `COMMAND_CHANNEL.try_receive()` is not polled. CDC commands sent during auto_scan are queued but not processed until the scan cycle completes. Fix: use `embassy_futures::select` to handle commands while scanning.
- **GetSettings/Trigger fail during auto_scan**: Same root cause as above — scanner task can't process CDC commands while awaiting scan result.
- ~~**PLLSAI1_Q breaks USB enumeration**~~: RESOLVED (2026-04-09). At 180MHz SYSCLK, PLLSAI_Q=DIV8 gives exact 48MHz and USB enumerates correctly via `mux::Clk48sel::PLLSAI1_Q`. The previous failure was at 168MHz where PLLSAI configuration was different.

## Async Display Black Screen (RESOLVED)

**Bug**: Embassy BSP's `DisplayCtrl::new()` produced a black screen. Sync BSP works with identical hardware. See [embassy-stm32f469i-disco#20](https://github.com/Amperstrand/embassy-stm32f469i-disco/issues/20).

### Resolution: `display_minimal` + `display_hybrid` HW verified (2026-04-09)

`display_minimal` shows 4 color bands (red/green/blue/white) in portrait 480x800. Mirrors the verified-working embassy example `examples/stm32f469/src/bin/dsi_bsp.rs` (commit `83e0d37`).

`display_hybrid` uses BSP fork's `DisplayCtrl::new()` API in embassy context — proves the BSP driver itself works, not just standalone DSI/LTDC code. HW verified.

### Root cause: BSP fork DSI timing and PLL config wrong

The BSP fork's `display.rs` had **multiple incorrect values** that diverged from the ST BSP and the working embassy example:

1. **DSI vertical timing completely wrong**: BSP used VSA=1/VBP=15/VFP=16. Working values are **VSA=120/VBP=150/VFP=150** (from ST BSP). These are raw line counts, not DSI lane clock cycles.
2. **DSI NULL_PACKET missing**: BSP set NULL_PACKET_SIZE=0. Working value is **0xFFF**.
3. **DSI VCCR NUMC wrong**: BSP set NUMC=1. Working value is **0**.
4. **DSI LPMCR wrong**: BSP used LPSIZE=64/VLPSIZE=64. Working values are **LPSIZE=16/VLPSIZE=0**.
5. **PLLSAI divq was incorrectly "fixed"**: We set `divq: Some(DIV8)` as a "fix" for CDC, but the working display example uses **`divq: None`**. The display and USB have different PLL requirements — this was a false fix.
6. **PLL SYSCLK**: Working display requires 180MHz (DIV8/MUL360). Our async firmware used 168MHz (DIV4/MUL168) for USB 48MHz.
7. **Pixel format**: Working example uses **RGB888 (DSI) + ARGB8888 (LTDC layer)**, not RGB565.
8. **Panel init**: Working example uses hardcoded DSI commands, not the nt35510 crate. The crate's `init_rgb565()` sends different sequences.
9. **No LP/HS mode switching needed**: The working example does NOT switch between AllInLowPower/AllInHighSpeed around panel init. The BSP fork added this incorrectly.

### Previous BSP fork fixes (still valid but insufficient)

These were fixed in BSP fork `972998f` and are still correct:
1. VMCR bit positions corrected
2. DSI horizontal timing calculation fixed (pixel→DSI lane cycle conversion)
3. LTDC GCR DEN bit added
4. CMCR LP/HS mode switching (unnecessary but harmless)

### Key learnings

- **DSI vertical timing values are RAW line counts**, not scaled to DSI lane byte clocks. Only horizontal timing (HSA, HBP, HLINE) needs scaling via `LANE_BYTE_CLK_KHZ / LCD_CLOCK_KHZ`.
- **PLLSAI config reconciled**: At 180MHz, PLLSAI_Q=DIV8 gives exact 48MHz for USB via `mux::Clk48sel::PLLSAI1_Q`. PLLSAI_R=DIV7 gives pixel clock for LTDC. PLL1_Q is NOT used for USB (it can't produce 48MHz at 180MHz SYSCLK). `divq: None` on PLL1 is fine — PLL1 provides SYSCLK and APB clocks only.
- **Raw `reg32_write()` vs stm32_metapac typed accessors**: This was the critical unexpected finding. Raw register writes that overwrite entire registers (including reserved bits) produced black screens even when the written values appeared correct. `stm32_metapac::LTDC` typed accessors (`.modify()`, `.write()`) work because they use read-modify-write, preserving reserved bits. Also, embassy's `init_layer()` had a CFBLL off-by-one bug for STM32F4: used `+7` instead of `+3` (per RM0090 §17.7.6).
- **The nt35510 crate may have incorrect init sequences** for embassy's DSI implementation. The hardcoded commands from the working example should be preferred.
- **180MHz SYSCLK is required** for display (PLLSAI pixel clock derivation). 168MHz (USB-optimized config) doesn't work.
- **Panel autodetection works**: Reading 0xDA/0xDB/0xDC via `DsiReadCommand` returns valid panel ID bytes after init.

### Files
- Working binary: `examples/stm32f469i-disco/src/bin/display_minimal.rs`
- BSP DisplayCtrl test: `examples/stm32f469i-disco/src/bin/display_hybrid.rs`
- Reference: [embassy dsi_bsp.rs](https://github.com/embassy-rs/embassy/blob/83e0d3780e42e3edf1f85d8ce75057baeb6927b4/examples/stm32f469/src/bin/dsi_bsp.rs) (commit `83e0d37`)
- BSP fork: `src/display.rs` at `/home/ubuntu/src/embassy-stm32f469i-disco/`
- ST BSP reference: [stm32469i_discovery_lcd.c](https://github.com/STMicroelectronics/32f469idiscovery-bsp/blob/main/stm32469i_discovery_lcd.c)

### Build + Flash
```bash
# display_minimal (standalone DSI/LTDC, defmt, no USB)
cargo build --release --target thumbv7em-none-eabihf \
  --manifest-path examples/stm32f469i-disco/Cargo.toml \
  --bin display_minimal --no-default-features --features scanner-async,defmt
arm-none-eabi-objcopy -O binary target/thumbv7em-none-eabihf/release/display_minimal /tmp/display_minimal.bin
st-flash --connect-under-reset write /tmp/display_minimal.bin 0x08000000
st-flash --connect-under-reset reset

# display_hybrid (BSP DisplayCtrl::new(), defmt, no USB)
cargo build --release --target thumbv7em-none-eabihf \
  --manifest-path examples/stm32f469i-disco/Cargo.toml \
  --bin display_hybrid --no-default-features --features scanner-async,defmt
```

## Future Work

- **RGB565 + RGB888 dual pixel format support** ([#21](https://github.com/Amperstrand/gm65-scanner/issues/21)): BSP currently hardcodes RGB888/ARGB8888. Future refactoring to support both formats (via generics, config enum, or separate examples). embedded-graphics natively favors RGB565. DMA throughput difference (2x) unlikely to matter at 60Hz with 480×800 panel.
- **nt35510 crate improvements** ([#22](https://github.com/Amperstrand/gm65-scanner/issues/22)): Crate's `init_rgb565()` has incorrect register values (B5/B6/B7/BA differ from ST BSP). Missing TEEON command. No RGB888 init sequence. No builder/raw API for custom init.

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
