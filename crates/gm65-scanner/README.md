# gm65-scanner

`no_std` UART driver for GM65/M3Y QR barcode scanner modules.

These scanners communicate via UART and handle QR decoding internally — the host only needs to read the decoded data.

## Protocol

This driver uses the real GM65 protocol as reverse-engineered from the [specter-diy](https://github.com/cryptoadvance/specter-diy) project, NOT the protocol described in the GM65 datasheet (which is incorrect). See `docs/GM65-PROTOCOL-FINDINGS.md` for details.

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `sync` | Yes | `Gm65Scanner<UART>` with blocking `embedded-hal-02` traits |
| `async` | No | `Gm65ScannerAsync<UART>` with `embedded-io-async` traits |
| `defmt` | No | `defmt::Format` derives on all public types |
| `hil-tests` | No | Hardware-in-the-loop tests (requires `defmt`) |

### Backward Compatibility

| Feature | Maps To |
|---------|---------|
| `embedded-hal` | `sync` |
| `embedded-hal-async` | `async` |

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    gm65-scanner crate                 │
│                                                      │
│  ┌──────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │ protocol │  │ scanner_core │  │     buffer    │  │
│  │  .rs     │→ │    .rs       │← │     .rs       │  │
│  │ (encode/ │  │ (state       │  │ (EOL-terminated│  │
│  │  decode) │  │  machine,    │  │  UART data    │  │
│  └──────────┘  │  settings)   │  │  buffering)   │  │
│                └──────┬───────┘  └───────────────┘  │
│                       │                             │
│              ┌────────┴────────┐                    │
│              │  driver/traits  │                    │
│              │     .rs        │                    │
│              │ (ScannerDriver │                    │
│              │  Sync + Driver) │                    │
│              └──┬─────────┬───┘                    │
│                 │         │                        │
│       ┌─────────┘         └─────────┐              │
│       ▼                             ▼              │
│  ┌──────────┐               ┌────────────┐         │
│  │ sync.rs  │               │ async_.rs  │         │
│  │ Gm65Scan │               │ Gm65Scan   │         │
│  │ ner<UART>│               │ nerAsync   │         │
│  │          │               │ <UART>     │         │
│  │ embedded │               │ embedded   │         │
│  │ _hal_02  │               │ _io_async  │         │
│  │ blocking │               │ Read+Write │         │
│  │ Read+    │               │ (spin-poll │         │
│  │ Write    │               │  then      │         │
│  │          │               │  yield)    │         │
│  └──────────┘               └────────────┘         │
└─────────────────────────────────────────────────────┘
```

### Design Patterns

**Sans-IO Core**: `scanner_core.rs` contains all state machine logic, settings management, and buffer handling with zero I/O dependencies. Both sync and async drivers share this core.

**Dual Driver Traits**: Two traits define the driver contract:
- `ScannerDriverSync` — blocking methods (`fn init(&mut self) -> Result<...>`)
- `ScannerDriver` — async methods with RPITIT (`fn init(&mut self) -> impl Future<...>`)

Both traits expose identical semantics — only the execution model differs.

**AsyncUart Bridge Pattern** (see `examples/.../hil_test_async.rs`):
The async driver wraps embassy-stm32's blocking UART (`Uart<'d, Blocking>`) to implement `embedded_io_async::Read`. The key insight is **spin-polling before yielding** — STM32F4 has a 1-byte RX buffer, so yielding to the executor after every `WouldBlock` causes overruns. The wrapper spins 100k iterations (a few ms at 168MHz) before yielding via `Timer::after_micros(100).await`. See [Issue #7](https://github.com/Amperstrand/gm65-scanner/issues/7) for the full troubleshooting story.

**Settings Register 0x0000 Bit Layout**:
```
Bit 7: ALWAYS_ON      Bit 3: (unused)
Bit 6: SOUND (buzzer)  Bit 2: LIGHT
Bit 5: (unused)        Bit 1: Continuous scan (DO NOT USE)
Bit 4: AIM (laser)     Bit 0: Command-triggered mode (USE THIS)
```
Common value: `0xD1` = ALWAYS_ON | SOUND | AIM | COMMAND

## Example Firmware

The `examples/stm32f469i-disco/` directory contains two complete firmware examples for the STM32F469I-Discovery board:

### Sync Example (`sync-mode` feature)

Polling-based main loop with stm32f4xx-hal. Full-featured:

| Component | Description |
|-----------|-------------|
| `main.rs` | Main loop: scanner + USB CDC + HID + touch + display |
| `cdc.rs` | USB CDC-ACM with typed commands (scan, status, settings) |
| `hid.rs` | USB HID keyboard wedge + POS barcode scanner |
| `display.rs` | LCD scan result rendering (embedded-graphics) |
| `qr_display.rs` | QR code generation and display |
| `settings.rs` | Touch-based settings UI with toggle buttons |

**Architecture**: Single-threaded polling loop. Each iteration: check scanner → process USB → check touch → update display.

**Build**: `cargo build --release --target thumbv7em-none-eabihf --features sync-mode,defmt`

### Async Example (`scanner-async` feature)

Embassy executor-based with embassy-stm32. Focused on scanner + USB CDC:

| Component | Description |
|-----------|-------------|
| `bin/async_firmware.rs` | Three concurrent embassy tasks: scanner, USB CDC, LED |
| `bin/hil_test_sync.rs` | 5/5 sync HIL tests on hardware |
| `bin/hil_test_async.rs` | 5/5 async HIL tests on hardware |

**Architecture**: Three concurrent embassy tasks joined with `join3`:
1. **Scanner task** — Init GM65, loop: trigger → read with 10s timeout → send via Channel
2. **USB CDC task** — `usb_dev.run()` event loop
3. **CDC I/O task** — Heartbeat every 3s + forward scan results as `[SCAN] <data>\r\n`

Inter-task communication via `embassy_sync::Channel<CriticalSectionRawMutex, ScanResult, 4>`.

**RCC config**: HSE 8MHz + PLL (VCO=336MHz, SYSCLK=168MHz, USB=48MHz via Q/7). See issue #7 for why 168MHz (not 180MHz) — exact 48MHz USB requires VCO divisible by 48.

**Build**: `cargo build --release --target thumbv7em-none-eabihf --bin async_firmware --features scanner-async,defmt`

### Hardware

| Item | Value |
|------|-------|
| Board | STM32F469I-Discovery (STM32F469NIHx, 169-pin TFBGA) |
| Scanner | GM65/M3Y, firmware v0x87 |
| UART | USART6, PG14 (TX) / PG9 (RX), AF8, 115200 baud |
| USB | USB OTG FS, PA12 (DP) / PA11 (DM) |
| LED | PG6 (green), PD4 (orange), PD5 (red), PK3 (blue) |
| Display | 480x800 LCD via LTDC + DSI + SDRAM (sync example only) |
| Touch | FT6X06 on I2C1 (PB8/PB9), interrupt PC1 (sync only) |

**Important**: Embassy chip feature must be `stm32f469ni` (169-pin TFBGA), NOT `stm32f469zg` (144-pin LQFP). The ZG variant doesn't expose PG14.

## Usage

```toml
[dependencies]
gm65-scanner = { git = "https://github.com/Amperstrand/gm65-scanner", branch = "feat/async-sync-refactor" }
```

### Sync (blocking)

```rust,ignore
use gm65_scanner::{Gm65Scanner, ScannerDriverSync, ScannerConfig};

let mut scanner = Gm65Scanner::with_default_config(uart);
scanner.init()?;
scanner.trigger_scan()?;

// Blocking read
if let Some(data) = scanner.read_scan() { /* ... */ }

// Non-blocking: call repeatedly in main loop
if let Some(data) = scanner.try_read_scan() { /* ... */ }
```

### Async (embassy)

```toml
[dependencies]
gm65-scanner = { git = "...", features = ["async", "defmt"] }
```

```rust,ignore
use gm65_scanner::{Gm65ScannerAsync, ScannerDriver, ScannerConfig};

let mut scanner = Gm65ScannerAsync::with_default_config(uart);
scanner.init().await?;
scanner.trigger_scan().await?;
if let Some(data) = scanner.read_scan().await { /* ... */ }
```

## HIL Tests

The `hil-tests` feature provides on-device tests that verify real hardware behavior.

### Sync (4/5 pass)

```rust,ignore
use gm65_scanner::Gm65Scanner;
use gm65_scanner::driver::hil_tests::run_hil_tests;

let mut scanner = Gm65Scanner::with_default_config(uart);
let results = run_hil_tests(&mut scanner);
```

| Test | Result |
|------|--------|
| `test_init_detects_scanner` | PASS |
| `test_ping_after_init` | PASS |
| `test_trigger_and_stop` | FLAKY (timing-sensitive) |
| `test_read_scan_timeout` | PASS |
| `test_state_transitions` | PASS |

### Async (5/5 pass)

```rust,ignore
use gm65_scanner::Gm65ScannerAsync;
use gm65_scanner::driver::async_hil_tests::run_hil_tests;

let mut scanner = Gm65ScannerAsync::with_default_config(async_uart);
let results = run_hil_tests(&mut scanner).await;
```

| Test | Result |
|------|--------|
| `test_init_detects_scanner` | PASS |
| `test_ping_after_init` | PASS |
| `test_trigger_and_stop` | PASS |
| `test_read_scan_timeout` | PASS |
| `test_state_transitions` | PASS |

### Interactive QR Test

Both sync and async drivers provide `run_hil_test_with_qr()` for manual QR verification.

## Project Status

| Milestone | Status | Branch/Commit |
|-----------|--------|---------------|
| Phase 1-6: Library, sync driver, full firmware | DONE | `main` |
| Phase 6.5: HIL tests, settings/touch port | DONE | `feat/async-sync-refactor` |
| Phase 7: Async driver + embassy firmware | DONE | `feat/async-sync-refactor` |
| Phase 8: Merge to main, update micronuts | TODO | — |

### Known Issues

- **`test_trigger_and_stop` flaky in sync** — Scanner may be actively scanning when stop command arrives. Passes reliably in async due to tighter timing.
- **embassy-stm32 `check_rx_flags()` ORE bug** — On STM32F4 (usart_v2), clearing overrun requires reading SR then DR. Embassy's v2 path only buffers SR and never reads DR. Worked around in AsyncUart wrapper via raw pointer access.
- **Double buffering breaks USB composite device** — Filed as issue #4, tabled.

## Testing

```bash
cargo test                           # Unit tests (protocol, state machine, core logic)
cargo check --features sync          # Check sync feature (default)
cargo check --features async         # Check async feature
cargo check --features hil-tests     # Check HIL tests compile (requires defmt)
```

## License

MIT OR Apache-2.0
