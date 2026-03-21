# gm65-scanner

No-std Rust driver for GM65/M3Y QR barcode scanner modules.

## Build

```bash
cargo build
cargo test
```

## Architecture

- `protocol.rs` - GM65 command/response protocol. Uses the REAL protocol reverse-engineered from specter-diy, NOT the datasheet protocol.
- `driver.rs` - ScannerDriver trait, ScannerConfig, error types, state machine
- `buffer.rs` - EOL-terminated UART data buffering
- `decoder.rs` - QR payload classification (Cashu V4, UR, URL, plain text)

## Key Design Decisions

1. **Synchronous trait**: `ScannerDriver` uses sync methods, not async. The scanner communication is inherently blocking (poll UART for response bytes). Users can wrap in async if needed.
2. **Protocol correctness**: The GM65 datasheet protocol is WRONG. Responses are `02 00 00 01 [value] 33 31` (7 bytes), commands end with `AB CD` sentinel (not a real CRC). See `protocol.rs` module docs.
3. **Register addresses**: Use specter-diy's addresses (e.g., SERIAL_ADDR = `[0x00, 0x0D]`), not the datasheet's. They differ.
4. **No embedded-hal in core**: The `ScannerDriver` trait is generic. HAL-specific implementations live in consumer crates (e.g., firmware/ in micronuts).

## Testing

```bash
cargo test
```

Tests verify:
- Command frame construction matches specter-diy bytes exactly
- Response parsing handles 7-byte success format
- Register address bytes match specter-diy constants

## Publishing

```bash
cargo publish
```

## References

- specter-diy qr.py: https://github.com/cryptoadvance/specter-diy/blob/master/src/hosts/qr.py
- GM65 datasheet: https://www.waveshare.net/wiki/GM65_Barcode_Scanner_Module (WARNING: protocol section is incorrect)
