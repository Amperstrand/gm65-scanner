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
| `hil-tests` | No | Hardware-in-the-loop tests (requires async + defmt) |

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

## Architecture

The crate uses a Sans-IO pattern:

- **Protocol layer** (`protocol.rs`): Pure command/response encoding, no I/O
- **State machine** (`state_machine.rs`): Configuration sequence, settings logic
- **Transport layer**: Separate sync (`sync.rs`) and async (`async_.rs`) implementations

Both `Gm65Scanner` and `Gm65ScannerAsync` share the same protocol and state machine code — only the I/O primitives differ.

## Hardware Verified

- **Board**: STM32F469I-Discovery with specter-diy shield-lite adapter
- **Scanner**: GM65 module, firmware v0.87, via USART6 (PG14 TX / PG9 RX)
- **Baud**: 9600 (default), 115200 supported
- **Mode**: Command-triggered scan, QR-only

## Testing

```bash
# Unit tests (protocol, state machine)
cargo test

# Check sync feature (default)
cargo check

# Check async feature
cargo check --features async

# Check HIL tests (requires hardware)
cargo check --features hil-tests
```

## License

MIT OR Apache-2.0
