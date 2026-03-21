# gm65-scanner

No-std Rust driver for GM65/M3Y QR barcode scanner modules.

## Build

```bash
cargo build
cargo test
```

## Architecture

- `protocol.rs` - GM65 command/response protocol. Uses the REAL protocol reverse-engineered from specter-diy, NOT the datasheet protocol. Includes `Register` enum, `Gm65Response` parser, command builders.
- `driver.rs` - `ScannerDriverSync` (blocking) and `ScannerDriver` (async) traits, `ScannerConfig`, error types, state machine
- `buffer.rs` - EOL-terminated UART data buffering, canonical home of `MAX_SCAN_SIZE`
- `scanner.rs` - `Gm65Scanner<UART>` concrete impl (behind `embedded-hal` feature). Uses `Register` enum + `Gm65Response::parse()` throughout.
- `decoder.rs` - Generic QR payload classification (`PayloadType`), `ParsedUrFragment`, `parse_ur_fragment()`, `UrDecoder` (no Cashu dependency)
- `lib.rs` - Re-exports all public API types

## Key Design Decisions

1. **Dual-mode traits**: `ScannerDriverSync` for polling main loops, `ScannerDriver` (async via `core::future::ready` wrapping sync) for executor-based firmware. Both implemented by `Gm65Scanner<UART>`.
2. **Protocol correctness**: The GM65 datasheet protocol is WRONG. Responses are `02 00 00 01 [value] 33 31` (7 bytes), commands end with `AB CD` sentinel (not a real CRC). See `docs/GM65-PROTOCOL-FINDINGS.md`.
3. **Register enum**: All register addresses are in the `Register` enum with `.address_bytes()` method. No raw `[u8; 2]` address literals outside `protocol.rs`.
4. **Gm65Response parser**: Centralized in `protocol.rs::Gm65Response::parse()`. `scanner.rs` uses `Option<Gm65Response>` return from `send_command()`.
5. **Generic UR decoder**: `UrDecoder` in `decoder.rs` is Cashu-agnostic — it handles any UR multi-fragment protocol. Cashu-specific decoding lives in consumer crates.
6. **embedded-hal feature**: Required for `Gm65Scanner<UART>` impl. The trait definitions in `driver.rs` are always available.
7. **embedded-hal-async feature**: Wraps sync methods in `core::future::ready()` — no real async I/O.

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
- Protocol findings: `docs/GM65-PROTOCOL-FINDINGS.md`
