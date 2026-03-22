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

## Usage

```toml
[dependencies]
gm65-scanner = { git = "https://github.com/Amperstrand/gm65-scanner", branch = "feat/async-sync-refactor" }
```

### Sync (blocking)

For polling-based main loops without an async executor:

```rust,ignore
use gm65_scanner::{Gm65Scanner, ScannerDriverSync, ScannerConfig};

let mut scanner = Gm65Scanner::with_default_config(uart);
scanner.init()?;
scanner.trigger_scan()?;

// Blocking read (avoid in main loops)
if let Some(data) = scanner.read_scan() { /* ... */ }

// Non-blocking: call repeatedly in main loop
if let Some(data) = scanner.try_read_scan() { /* ... */ }
```

### Async (embassy, embedded-executor, etc.)

For executor-based async firmware:

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

// Non-blocking polling
if let Some(data) = scanner.try_read_scan().await { /* ... */ }
```

## Hardware-In-the-Loop (HIL) Tests

The `hil-tests` feature provides on-device tests that verify real hardware behavior. These tests run on actual GM65 hardware connected via UART.

### Prerequisites

1. GM65/M3Y scanner connected to UART
2. `defmt` logger configured (required for test output)
3. Hardware targeting your platform (STM32, nRF, etc.)

### Running Sync HIL Tests

```rust,ignore
use gm65_scanner::Gm65Scanner;
use gm65_scanner::driver::sync::hil_tests::run_hil_tests;

// Initialize UART with your HAL (embedded-hal 0.2.x blocking traits)
let uart = /* your UART peripheral */;

// Create scanner andlet mut scanner = Gm65Scanner::with_default_config(uart);

// Run all 5 HIL tests
let results = run_hil_tests(&mut scanner);

// Check results
if results.all_passed() {
    defmt::info!("All HIL tests passed!");
} else {
    defmt::error!("HIL tests failed: {}/5 passed", results.passed_count());
}
```

### Running Async HIL Tests

```rust,ignore
use gm65_scanner::Gm65ScannerAsync;
use gm65_scanner::driver::async_::hil_tests::run_hil_tests;

// Initialize async UART with your HAL (embedded-io-async traits)
let uart = /* your async UART peripheral */;

// Create scanner
let mut scanner = Gm65ScannerAsync::with_default_config(uart);

// Run all 5 HIL tests (async)
let results = run_hil_tests(&mut scanner).await;

// Check results
if results.all_passed() {
    defmt::info!("All HIL tests passed!");
} else {
    defmt::error!("HIL tests failed: {}/5 passed", results.passed_count());
}
```

### Test Results Structure

```rust,ignore
pub struct HilTestResults {
    pub init_detects_scanner: bool,  // Scanner responds to init
    pub ping_after_init: bool,       // Ping works after init
    pub trigger_and_stop: bool,      // Trigger/stop scan cycle works
    pub read_scan_timeout: bool,     // Read times out correctly
    pub state_transitions: bool,     // State machine transitions correctly
}
```

### Interactive QR Test

For manual verification with a real QR code:

```rust,ignore
// Sync version
use gm65_scanner::driver::sync::hil_tests::run_hil_test_with_qr;
let passed = run_hil_test_with_qr(&mut scanner);

// Async version
use gm65_scanner::driver::async_::hil_tests::run_hil_test_with_qr;
let passed = run_hil_test_with_qr(&mut scanner).await;
```

## Architecture

The crate uses a Sans-IO pattern:

- **Core layer** (`scanner_core.rs`): State machine, buffer management, shared types
- **Protocol layer** (`protocol.rs`): Pure command/response encoding, no I/O
- **Transport layer**: Separate sync (`sync.rs`) and async (`async_.rs`) implementations

Both `Gm65Scanner` and `Gm65ScannerAsync` share the same core logic — only the I/O primitives differ.

## Hardware Verified

- **Board**: STM32F469I-Discovery with specter-diy shield-lite adapter
- **Scanner**: GM65 module, firmware v0.87, via USART6 (PG14 TX / PG9 RX)
- **Baud**: 9600 (default), 115200 supported
- **Mode**: Command-triggered scan, QR-only

## Testing

```bash
# Unit tests (protocol, state machine, core logic)
cargo test

# Check sync feature (default)
cargo check

# Check async feature
cargo check --features async

# Check HIL tests compile (requires defmt)
cargo check --features hil-tests
```

## License

MIT OR Apache-2.0
