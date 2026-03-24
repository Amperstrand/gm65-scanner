# gm65-scanner

`no_std` Rust driver for GM65/M3Y QR barcode scanner modules with firmware examples.

## Overview

This project provides a complete scanner solution:

- **Library** (`crates/gm65-scanner/`) — Sans-IO core with sync and async drivers
- **Firmware** (`examples/stm32f469i-disco/`) — Full scanner application for STM32F469I-Discovery board

## Features

| Feature | Description |
|---------|-------------|
| Sync driver | `Gm65Scanner<UART>` with `embedded-hal-02` traits |
| Async driver | `Gm65ScannerAsync<UART>` with `embedded-io-async` traits |
| HIL tests | Hardware-in-the-loop tests for both drivers |
| QR display | Generate and display QR codes on LCD |
| USB CDC | Host control via virtual serial port |
| USB HID | Keyboard wedge and barcode scanner HID modes |

## Project Status

| Component | Status | Notes |
|-----------|--------|-------|
| Library | ✅ Stable | 35 unit tests passing |
| Sync firmware | ✅ Working | Full features: scanner, display, touch, USB CDC/HID |
| Async firmware | ✅ Working | Embassy executor, concurrent tasks |
| HIL tests | ✅ Passing | Both sync and async drivers validated |

## Dependencies (Pinned)

| Dependency | Version | Notes |
|------------|---------|-------|
| `stm32f469i-disc` | git `fa6dc86` | Amperstrand BSP fork |
| `embassy-*` | git `84444a19` | Embassy framework (Monorepo) |
| `qrcodegen-no-heap` | 1.8 | QR code generation (zero heap) |
| `embedded-hal` | 1.0 | HAL traits |
| `embedded-hal-02` | 0.2 | Legacy HAL traits (sync driver) |
| `embedded-io-async` | 0.7 | Async I/O traits |

## Testing

### Unit Tests

```bash
cargo test -p gm65-scanner --lib
```

**Status**: ✅ 35/35 tests passing

### Hardware-in-the-Loop (HIL) Tests

Flash to STM32F469I-Discovery board:

```bash
# Sync HIL tests
cargo flash --release --chip stm32f469nihx --bin hil_test_sync

# Async HIL tests  
cargo flash --release --chip stm32f469nihx --bin hil_test_async --features scanner-async
```

**Status**: ⚠️ Requires hardware - not yet validated on latest build

### Build Verification

```bash
# Sync firmware
cargo build --release --target thumbv7em-none-eabihf

# Async firmware
cargo build --release --target thumbv7em-none-eabihf --features scanner-async
```

**Status**: ✅ Both compile successfully

## Before Release

The following items need validation before tagging a release:

1. **Hardware Testing**
   - [ ] Flash `hil_test_sync` to board, verify 5/5 tests pass
   - [ ] Flash `hil_test_async` to board, verify 5/5 tests pass
   - [ ] Test USB CDC communication with host software
   - [ ] Test QR scanning and display rendering

2. **Documentation**
   - [x] Add API documentation (cargo doc)
   - [ ] Document CDC protocol commands

3. **Integration Testing**
   - [ ] End-to-end scan → display → USB CDC flow
   - [ ] Touch input and settings screen interaction
   - [ ] USB HID keyboard wedge functionality

4. **BSP Update (Optional)**
   - [ ] Evaluate updating `stm32f469i-disc` to latest version
   - [ ] Test with updated BSP before merging

## CDC Protocol

The firmware exposes a USB CDC serial interface with these commands:

| Command | Code | Description |
|---------|------|-------------|
| ScannerStatus | 0x10 | Get scanner connection status |
| ScannerTrigger | 0x11 | Trigger a scan |
| ScannerData | 0x12 | Read last scan data |
| GetSettings | 0x13 | Read scanner settings |
| SetSettings | 0x14 | Write scanner settings |
| DisplayQr | 0x15 | Display QR code on LCD |
| EnterSettings | 0x16 | Enter settings screen |

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                 gm65-scanner workspace                 │
│                                                    │
│  ┌────────────────┐      ┌────────────────────┐    │
│  │ crates/gm65-  │      │ examples/         │    │
│  │ scanner/       │      │ stm32f469i-disco/ │    │
│  │                │      │                   │    │
│  │ Sans-IO core  │      │ Firmware binary  │    │
│  │ Sync driver    │      │ Sync (HAL)        │    │
│  │ Async driver   │      │ Async (Embassy)   │    │
│  └────────────────┘      └────────────────────┘    │
└─────────────────────────────────────────────────────┘
```

## Branches

| Branch | Purpose | Action |
|--------|---------|--------|
| `main` | Active development | Current work |
| `origin/old-main` | Preserved original main | Archive - delete after confirming |
| `origin/explore/lcd-gram-retention` | LCD GRAM test PoC | Experimental - keep or delete |

## License

MIT OR Apache-2.0

## Resources

- [GM65 Protocol Findings](crates/gm65-scanner/docs/GM65-PROTOCOL-FINDINGS.md)
- [Crate Documentation](crates/gm65-scanner/README.md)
