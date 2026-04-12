# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-04-12

### Added

- RGB888/ARGB8888 display pixel format support (migrated from RGB565)
- Touch-driven settings UI for sync firmware with visual buttons and larger tap targets
- Heartbeat LED blink task for async firmware (3-second cycle)
- Embassy display integration using nt35510 crate for panel init
- FT6X06 touch support via I2C1 (PB8/PB9) with identity coordinate transform
- NT35510 hardware test binary for panel validation
- Touch calibration test binary (6 target rectangles, hardware verified)
- `usb_minimal` diagnostic binary for standalone USB CDC testing
- `nt35510_hwtest` diagnostic binary
- `embassy_display_bsp_minimal` diagnostic binary with corrected NT35510 init
- `async_display_test` diagnostic binary
- 180MHz SYSCLK with PLLSAI-derived 48MHz USB clock and 54.86MHz LTDC pixel clock
- USB CDC host control protocol with 3-byte framed commands (ScannerStatus, Trigger, Data, GetSettings, SetSettings, DisplayQr)
- `ScannerDriver` async trait and `Gm65ScannerAsync<UART>` implementation
- `ScannerDriverSync` trait and `Gm65Scanner<UART>` implementation
- Sans-IO `ScannerCore` state machine shared between sync and async drivers
- `InitAction` state machine for multi-step scanner initialization
- `UrDecoder` for generic multi-part UR fragment reassembly (Cashu-agnostic)
- `PayloadType` classification for QR payload content detection
- `#[must_use]` annotations on all public API methods and types
- `defmt` feature for embedded logging on all public types
- `hil-tests` feature with 6 sync and 9 async on-device tests, all hardware verified
- Batch-drain `try_read_scan` method on sync driver
- `get_setting` / `set_setting` made public on async driver
- QR code rendering on LCD with centered display and theme colors
- Auto-scan mode with continuous scanning in production firmware
- LED blink feedback on successful QR scan (sync firmware)
- CDC response timeout and QR render yield points in async firmware
- Orientation support via BSP pin update

### Changed

- Decomposed async firmware from monolithic function into 8 focused modules, fixing 20+ silent channel drops
- Decomposed sync firmware `main()` into `init_hardware()` and `run_main_loop()`
- Extracted shared `scanner_utils` module to DRY sync and async firmware
- Extracted theme color constants from display code
- Replaced magic numbers with named constants across display, qr_display, cdc, and display_utils
- Pinned all dependencies to specific GitHub commits (embassy, nt35510, BSPs)
- Moved BSP-level diagnostic binaries out to their respective BSP fork repos
- Consolidated embassy-stm32f469i-disco examples from 29 to 8
- Removed `std` feature from library crate (unused)
- Removed debug-only features from default feature set
- Removed blanket `allow(dead_code)`, added targeted allows only where needed
- Updated nt35510 pin to v0.2.0 publish prep revision
- Aligned stm32f469i-disc BSP pin (orientation support, defmt removed from default)
- Aligned embassy-stm32f469i-disco BSP pin (TouchError + DisplayInitError types)

### Fixed

- 180MHz scanner init failure caused by LTDC ISR not clearing flags, fixed with task gating behind SCANNER_INIT_DONE signal
- Async CDC data flow failure with five independent root causes:
  - Removed broken `select(usb_dev.run(), scanner.init())` pattern that left USB bus in invalid state
  - Fixed `AsyncUart::read()` busy-poll (500K spins) starving USB in cooperative executor by yielding on every `WouldBlock`
  - Fixed CDC task channel race where `try_receive()` polled before scanner processed command, switched to `receive().await`
  - Removed `[ALIVE]` heartbeat that corrupted protocol framing every 3 seconds
  - Fixed mutex guards held across `.await` causing deadlocks
- Double `USART6.disable()` call causing undefined behavior on STM32F469
- Heap/framebuffer overlap causing allocator corruption (framebuffer uses `u32` not `u16`, 1.5MB not 768KB)
- `defmt_rtt` preventing USB OTG FS enumeration by disabling interrupts during critical timing
- `defmt.x` linker script shadowing in build.rs
- `drain_uart()` silently discarding in-flight scan data (now skipped when in `Scanning` state)
- 17 build errors during RGB888 migration
- QR size validation to prevent buffer overflows on display
- CDC retry limit and hex table constants
- Missing `[[bin]]` entries with required-features in Cargo.toml
- Async firmware force-write settings alignment with sync driver behavior
- `truncate_str()` panic on multi-byte UTF-8 boundary
- `UrDecoder::feed()` underflow when index=0
- Async scanner init USB ordering, reduced AsyncUart yield frequency
- Async scanner task select, stop_scan before settings queries, CDC response length
- Async touch using incorrect I2C1 pins (fixed to PB8/PB9)
- Portrait layout for touch settings UI
- Sync firmware always starting on home screen
- Duration typo in async firmware
- `build_save_settings()` sentinel documented with `SAVE_SENTINEL` constant
- Stale commit hashes, broken RTT targets, and outdated docs
- README cross-compile commands using `-p` which ignores `--no-default-features`
- Makefile and README build commands including defmt, breaking USB CDC
- Clippy warnings across library and firmware

## [0.1.0] - 2026-03-21

### Added

- Initial QR code module foundation
- GM65 UART protocol implementation based on specter-diy reverse-engineering (not datasheet)
- `Register` enum with `.address_bytes()` for all GM65 register addresses
- `Gm65Response` parser for centralized response handling
- Command frame builders for GET/SET register operations
- Sync scanner driver with UART communication at 115200 baud
- USB CDC serial interface for host communication
- USB HID composite device (CDC + keyboard wedge + POS scanner)
- QR code display on LCD via DSI/LTDC (NT35510)
- LCD mirroring support
- Workspace restructure with example firmware targeting STM32F469I-Discovery
- GitHub Actions CI workflow with clippy and cross-compile checks
- `rust-version` minimum of 1.75 and documentation link in crate metadata

### Fixed

- GM65 protocol corrected based on specter-diy findings (datasheet protocol is incorrect)
- LCD dimensions corrected for centered QR display
- HIL heap initialization and scan loop issues
- Phase 6 settings/touch port test robustness
- Removed `defmt::Format` from `ParsedUrFragment` (String/Vec<u8> not compatible)
- Syntax errors and unused imports from initial development
- Simulated scan injection removed (testing artifact)
- Duplicate `init()` call in sync HIL test causing NotDetected error

[0.2.0]: https://github.com/Amperstrand/gm65-scanner/compare/v0.1.0...ce4969a
[0.1.0]: https://github.com/Amperstrand/gm65-scanner/releases/tag/v0.1.0
