# gm65-scanner

`no_std` Rust driver for GM65/M3Y QR barcode scanner modules with firmware examples.

## Overview

- **Library** (`crates/gm65-scanner/`) — Sans-IO core with sync and async drivers, 88 unit tests
- **Firmware** (`examples/stm32f469i-disco/`) — Scanner application for STM32F469I-Discovery board

## Features

| Feature | Description |
|---------|-------------|
| Sync driver | `Gm65Scanner<UART>` with `embedded-hal-02` traits |
| Async driver | `Gm65ScannerAsync<UART>` with `embedded-io-async` traits |
| HIL tests | Hardware-in-the-loop tests for both drivers (sync: 5 core + 1 QR, async: 5 core + 3 extended + 1 QR) |
| QR display | Generate and display QR codes on LCD |
| USB CDC | Host control via virtual serial port |

## Project Status

| Component | Status | Notes |
|-----------|--------|-------|
| Library | Stable | 88 unit tests passing, clippy clean |
| Sync firmware | Working | Scanner + USB CDC + LCD display + QR rendering |
| Async firmware | Working | Embassy executor, concurrent tasks, LCD, USB CDC |
| HIL tests (sync) | 5/5 core HW verified | QR scan test not yet verified on hardware |
| HIL tests (async) | 8/9 HW verified | Core + extended pass; QR scan with visual feedback pending flash |

## Dependencies (Pinned)

| Dependency | Version | Notes |
|------------|---------|-------|
| `stm32f469i-disc` | git `9f52a58` | Amperstrand BSP fork (sync) |
| `embassy-stm32f469i-disco` | git `890a4d1` | Amperstrand BSP fork (async) |
| `embassy-*` | git `84444a19` | Embassy framework |
| `qrcodegen-no-heap` | 1.8 | QR code generation (zero heap) |
| `embedded-hal` | 1.0 | HAL traits |
| `embedded-hal-02` | 0.2 | Legacy HAL traits (sync driver) |
| `embedded-io-async` | 0.7 | Async I/O traits |

## Testing

### Unit Tests (no hardware required)

```bash
cargo test -p gm65-scanner --lib
```

**Status**: 88/88 tests passing

### Feature Checks

```bash
cargo check -p gm65-scanner              # sync (default)
cargo check -p gm65-scanner --features async
cargo check -p gm65-scanner --features defmt
cargo check -p gm65-scanner --features async,defmt
cargo check -p gm65-scanner --features std
```

### Hardware-in-the-Loop (HIL) Tests

Flash to STM32F469I-Discovery board:

```bash
# Sync HIL tests (5 core + QR scan)
make run-sync

# Async HIL tests (5 core + 3 extended + QR scan with LED/aim laser)
make run-async
```

### CDC Protocol Tests

```bash
# Flash firmware and run host-side CDC protocol tests
make test-sync
make test-async
```

## Build

```bash
# Sync firmware
make build-sync

# Async firmware
make build-async

# Cross-compile for ARM
cargo build --release --target thumbv7em-none-eabihf -p stm32f469i-disco-scanner --features sync-mode,defmt
cargo build --release --target thumbv7em-none-eabihf -p stm32f469i-disco-scanner --features scanner-async,defmt
```

## Binary Targets

| Binary | Description |
|--------|-------------|
| `stm32f469i-disco-scanner` (sync) | Full firmware: LCD, USB CDC, QR scanner, QR rendering, auto-scan |
| `async_firmware` | Embassy: LCD, USB CDC, QR scanner, LED, concurrent tasks |
| `hil_test_sync` | Sync HIL: 5 core tests + QR scan test, RTT output |
| `hil_test_async` | Async HIL: 5 core + 3 extended + QR scan with aim laser + LED blink, RTT output |

## CDC Protocol

The sync firmware exposes a USB CDC serial interface with these commands:

| Command | Code | Description |
|---------|------|-------------|
| ScannerStatus | 0x10 | Get scanner connection status |
| ScannerTrigger | 0x11 | Trigger a scan |
| ScannerData | 0x12 | Read last scan data |
| GetSettings | 0x13 | Read scanner settings |
| SetSettings | 0x14 | Write scanner settings |
| DisplayQr | 0x15 | Display QR code on LCD |

## Known Issues

### drain_uart() data loss (#12) — FIXED

`send_command()` now skips `drain_uart()` when the scanner is in `Scanning` state, preventing in-flight scan data from being silently discarded.

### BarType register non-persistent (#10)

GM65 firmware 0.87 silently rejects BarType (0x002C) writes while still ACKing. Not blocking — QR scanning works regardless via auto-detection.

### Settings 0x81 vs 0xD1 (#11)

0x81 (ALWAYS_ON | COMMAND) is the correct default. SOUND adds unwanted audible feedback, AIM is controlled programmatically.

### LCD GRAM retention (#5)

NT35510 internal GRAM retains previous frame for ~10s after power-cycle. Expected DRAM behavior, not a bug.

### Double-buffering breaks USB (#4)

LTDC `set_layer_buffer_address` + `reload_on_vblank` race condition breaks USB DMA. Single-buffer workaround in place.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                 gm65-scanner workspace                │
│                                                      │
│  ┌────────────────┐      ┌────────────────────┐     │
│  │ crates/gm65-   │      │ examples/          │     │
│  │ scanner/       │      │ stm32f469i-disco/  │     │
│  │                │      │                    │     │
│  │ protocol.rs    │      │ main.rs (sync fw)  │     │
│  │ scanner_core.rs│      │ async_firmware.rs  │     │
│  │ driver/sync.rs │      │ hil_test_sync.rs   │     │
│  │ driver/async_  │      │ hil_test_async.rs  │     │
│  │ buffer.rs      │      │ display.rs         │     │
│  │ decoder.rs     │      │ cdc.rs             │     │
│  └────────────────┘      └────────────────────┘     │
└─────────────────────────────────────────────────────┘
```

## License

MIT OR Apache-2.0

## Resources

- [GM65 Protocol Findings](crates/gm65-scanner/docs/GM65-PROTOCOL-FINDINGS.md)
- [Crate Documentation](crates/gm65-scanner/README.md)
