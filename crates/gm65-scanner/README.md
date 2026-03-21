# gm65-scanner

`no_std` UART driver for GM65/M3Y QR barcode scanner modules.

These scanners communicate via UART and handle QR decoding internally — the host only needs to read the decoded data.

## Protocol

This driver uses the real GM65 protocol as reverse-engineered from the [specter-diy](https://github.com/cryptoadvance/specter-diy) project, NOT the protocol described in the GM65 datasheet (which is incorrect). See `docs/GM65-PROTOCOL-FINDINGS.md` for details.

## Features

| Feature | Description |
|---------|-------------|
| `embedded-hal` | `Gm65Scanner<UART>` with `ScannerDriverSync` trait |
| `embedded-hal-async` | Also enable async `ScannerDriver` trait |
| `defmt` | `defmt::Format` derives on all public types |
| `std` | Standard library support |

## Usage

```toml
[dependencies]
gm65-scanner = { git = "https://github.com/Amperstrand/gm65-scanner", branch = "main", features = ["embedded-hal", "defmt"] }
```

### Sync (polling main loop)

```rust,ignore
use gm65_scanner::{Gm65Scanner, ScannerDriverSync, ScannerConfig};

let mut scanner = Gm65Scanner::with_default_config(uart);
scanner.init().ok();
scanner.trigger_scan().ok();

// Blocking read (avoids this in main loops — use try_read_scan instead)
if let Some(data) = scanner.read_scan() { /* ... */ }

// Non-blocking: call repeatedly in main loop
if let Some(data) = scanner.try_read_scan() { /* ... */ }
```

### Async

```rust,ignore
use gm65_scanner::{Gm65Scanner, ScannerDriver, ScannerConfig};

let mut scanner = Gm65Scanner::with_default_config(uart);
scanner.init().await.ok();
scanner.trigger_scan().await.ok();
if let Some(data) = scanner.read_scan().await { /* ... */ }
```

## Hardware Verified

- **Board**: STM32F469I-Discovery with specter-diy shield-lite adapter
- **Scanner**: GM65 module, firmware v0.87, via USART6 (PG14 TX / PG9 RX)
- **Baud**: 115200 (auto-probes 9600, 57600, 115200)
- **Mode**: Continuous scan, QR-only

## Testing

```bash
cargo test  # 6 protocol unit tests, mock-based
```

## License

MIT OR Apache-2.0
