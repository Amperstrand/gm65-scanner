# gm65-scanner

![crates.io](https://crates.io/crates/gm65-scanner)
![Downloads](https://img.shields.io/badge/draft%20?style=flat-square)](https://img.shields.io/badge.svg)

![License](https://img.shields.io/badge/License)](https://img.shields.io/badge/Apache-2.0)

![No Std](https://img.shields.io/badge/no_std)

![Embedded](https://img.shields.io/badge/embedded)

![Hardware Support](https://img.shields.io/badge/hardware-support)

![Status](https://img.shields.io/badge/status-active)

![GitHub](https://img.shields.io/github/followers/Amperstrand.svg?style=social&logo=github&height=20)](https://github.com/Amperstrand/gm65-scanner)
![Crates.io](https://crates.io/crates/gm65-scanner)

![Latest Version](https://img.shields.io/crates.io/v/0.1.0)
![License](https://img.shields.io/badge/license-MIT OR Apache-2.0-blue)

![Documentation](https://docs.rs/gm65-scanner/0.1.0)

![Repository](https://img.shields.io/badge/repository-GitHub)

![Build Status](https://img.shields.io/badge/build-passing-brightgreen)

![Version](https://img.shields.io/badge/version-v0.1.0-blue)
![License](https://img.shields.io/badge/license-MIT or Apache-2.0-green)

![No Std](https://img.shields.io/badge/no_std-yes)
![Embedded](https://img.shields.io/badge/embedded-yes)

## Overview

A `no_std` compatible driver for GM65 and M3Y QR barcode scanner modules. These scanners communicate via UART and handle QR decoding internally - the host only needs to read the decoded data.

- No external dependencies required for core functionality
- Optional `embedded-hal` support for hardware integration
- Optional `cashu` feature for Cashu token decoding
- Configurable baud rate, trigger mode, and RAW mode support
- Tested with mock implementations for host-based development

- Production-ready for embedded systems
- Supports multiple scanner models (GM65, M3Y, generic)
- Compatible with STM32, ESP32, and other embedded platforms
- Comprehensive protocol documentation included
- Full documentation and examples available at [docs.rs](https://docs.rs/gm65-scanner)
## Installation
Add to your `Cargo.toml`:
```toml
[dependencies]
gm65-scanner = "0.1"
```
## Basic Example
```rust
use gm65_scanner::{Gm65Scanner, ScannerConfig, ScanMode};

// For embedded-hal based usage
let config = ScannerConfig {
    baud_rate: 115200,
    mode: ScanMode::CommandTriggered,
    raw_mode: true,
};

let mut scanner = Gm65Scanner::new(uart, Some(trigger_pin), config);
scanner.init().await.ok();

// Trigger a scan
scanner.trigger_scan().await.ok();

// Read scanned data
if let Some(data) = scanner.read_scan().await {
    println!("Scanned: {:?}", data);
}
```

## Features
- **`embedded-hal`** - Enable embedded-hal trait implementations
- **`embedded-hal-async`** - Enable async embedded-hal support  
- **`cashu`** - Enable Cashu token decoding support
- **`std`** - Enable standard library support
- **`ur`** - Enable UR animated QR support
- **`defmt`** - Enable defmt logging support
