# Contributing to gm65-scanner

Thank you for your interest in contributing! This guide covers local development,
CI expectations, hardware-in-the-loop (HIL) testing, and code style.

## Getting Started

```bash
# Clone and build
git clone https://github.com/Amperstrand/gm65-scanner.git
cd gm65-scanner

# Run library tests (no hardware needed)
cargo test -p gm65-scanner --lib

# Check all feature combinations
cargo check -p gm65-scanner --all-features

# Lint
cargo fmt --all -- --check
cargo clippy -p gm65-scanner -- -D warnings
cargo clippy -p gm65-scanner --features async -- -D warnings
cargo clippy -p gm65-scanner --all-features -- -D warnings
```

## Project Structure

```
crates/gm65-scanner/       # no_std driver crate (publishable on crates.io)
examples/stm32f469i-disco/ # Firmware example for STM32F469I-Discovery board
scripts/                   # Flash/recovery helper scripts
```

The driver crate uses a **sans-IO core** (`ScannerCore`) shared between sync
(`embedded-hal 0.2`) and async (`embedded-io-async` + `embassy-time`) drivers.

## CI Expectations

All PRs must pass:

| Job | What it checks |
|-----|---------------|
| **Test** | `cargo test -p gm65-scanner --lib` |
| **Check Features** | All feature combinations compile |
| **Cross-compile** | Firmware builds for `thumbv7em-none-eabihf` |
| **Lint** | `cargo fmt --check` + `cargo clippy -D warnings` (multiple feature sets) |
| **Publish dry-run** | Crate manifest is valid |

## Making Changes

### Driver crate (`crates/gm65-scanner`)

- Maintain `no_std` compatibility — the crate must work without `std`.
- Keep the sans-IO core (`scanner_core.rs`) free of I/O operations.
- Add unit tests for new functionality using the mock UART helpers.
- Run `cargo clippy -p gm65-scanner --all-features -- -D warnings` before submitting.

### Firmware examples (`examples/stm32f469i-disco`)

- Cross-compile with: `cargo build --release --target thumbv7em-none-eabihf -p stm32f469i-disco-scanner --no-default-features --features <feature-set>`
- Do **not** add `defmt-rtt` or `panic-probe` to production firmware features
  (they prevent USB enumeration).

## HIL (Hardware-in-the-Loop) Testing

HIL tests require an STM32F469I-Discovery board with a GM65/M3Y scanner module.

```bash
# Build and run sync HIL tests (uses probe-rs RTT)
make run-sync

# Build and run async HIL tests
make run-async

# Test CDC protocol against running firmware
make test-cdc
```

See the [Makefile](Makefile) for all available targets.

## Code Style

- Follow standard Rust formatting (`cargo fmt`).
- Use `clippy` with `-D warnings`.
- Prefer `defmt` logging behind the `defmt` feature gate.
- Document public items with doc comments.
- Keep commit messages concise and descriptive.

## Protocol Notes

The GM65 UART protocol is **reverse-engineered** from
[specter-diy](https://github.com/cryptoadvance/specter-diy).
The official GM65 datasheet protocol description is incorrect.
See `crates/gm65-scanner/docs/GM65-PROTOCOL-FINDINGS.md` for details.

## Upstream Policy

**Never** file PRs or issues on upstream projects without human review.
See [#19](https://github.com/Amperstrand/micronuts/issues/19) for context.

## License

By contributing, you agree that your contributions will be licensed under
the project's dual MIT / Apache-2.0 license.
