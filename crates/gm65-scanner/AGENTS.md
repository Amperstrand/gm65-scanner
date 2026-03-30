# gm65-scanner

No-std Rust driver for GM65/M3Y QR barcode scanner modules.

## Build

```bash
cargo build
cargo test
```

## Architecture

- `protocol.rs` - GM65 command/response protocol. Uses the REAL protocol reverse-engineered from specter-diy, NOT the datasheet protocol. Includes `Register` enum, `Gm65Response` parser, command builders.
- `driver/mod.rs` - Driver module root, feature-gated sync/async submodules
- `driver/traits.rs` - `ScannerDriverSync` (blocking) and `ScannerDriver` (async) traits
- `driver/types.rs` - `ScannerConfig`, `ScannerError`, `ScannerModel`, `ScannerState`, `ScannerStatus`
- `driver/sync.rs` - `Gm65Scanner<UART>` concrete sync impl with mock UART tests
- `driver/async_.rs` - `Gm65ScannerAsync<UART>` concrete async impl
- `buffer.rs` - EOL-terminated UART data buffering, canonical home of `MAX_SCAN_SIZE`
- `scanner_core.rs` - `ScannerCore` state machine, `ScannerSettings` bitflags, init config sequence
- `decoder.rs` - Generic QR payload classification (`PayloadType`), `ParsedUrFragment`, `parse_ur_fragment()`, `UrDecoder` (no Cashu dependency)
- `lib.rs` - Re-exports all public API types

## Key Design Decisions

1. **Dual-mode traits**: `ScannerDriverSync` for polling main loops, `ScannerDriver` (async) for executor-based firmware. Both implemented by `Gm65Scanner<UART>` / `Gm65ScannerAsync<UART>`.
2. **Protocol correctness**: The GM65 datasheet protocol is WRONG. Responses are `02 00 00 01 [value] 33 31` (7 bytes), commands end with `AB CD` sentinel (not a real CRC). See `docs/GM65-PROTOCOL-FINDINGS.md`.
3. **Register enum**: All register addresses are in the `Register` enum with `.address_bytes()` method. No raw `[u8; 2]` address literals outside `protocol.rs`.
4. **Gm65Response parser**: Centralized in `protocol.rs::Gm65Response::parse()`. Drivers use `Option<Gm65Response>` return from `send_command()`.
5. **Generic UR decoder**: `UrDecoder` in `decoder.rs` is Cashu-agnostic — it handles any UR multi-fragment protocol. Cashu-specific decoding lives in consumer crates.
6. **embedded-hal-02 feature**: Required for sync `Gm65Scanner<UART>` impl. The trait definitions in `driver/traits.rs` are always available.
7. **embedded-io-async feature**: Required for async `Gm65ScannerAsync<UART>` impl.

## Testing

```bash
cargo test                          # sync tests only
cargo test --features async         # sync + async tests
cargo clippy -p gm65-scanner -- -D warnings
cargo clippy -p gm65-scanner --features async -- -D warnings
```

## Publishing

```bash
cargo publish
```

## References

- specter-diy qr.py: https://github.com/cryptoadvance/specter-diy/blob/master/src/hosts/qr.py
- GM65 datasheet: https://www.waveshare.net/wiki/GM65_Barcode_Scanner_Module (WARNING: protocol section is incorrect)
- Protocol findings: `docs/GM65-PROTOCOL-FINDINGS.md`
