//! GM65/M3Y QR Scanner Driver
//!
//! A `no_std` compatible driver for GM65 and M3Y QR barcode scanner modules.
//! These scanners communicate via UART and handle QR decoding internally.
//!
//! # Protocol
//!
//! This driver uses the real GM65 protocol as reverse-engineered from the
//! specter-diy project, NOT the protocol described in the GM65 datasheet
//! (which is incorrect). See `protocol.rs` for details.
//!
//! # Features
//!
//! - `sync` (default) — Enables `Gm65Scanner<UART>` with blocking I/O
//! - `async` — Enables `Gm65ScannerAsync<UART>` with async I/O
//! - `defmt` — Enables `defmt::Format` derives for logging
//!
//! # Sync vs Async
//!
//! Two driver implementations are provided:
//!
//! - `Gm65Scanner<UART>` — blocking methods, for polling main loops
//! - `Gm65ScannerAsync<UART>` — async methods, for executor-based firmware
//!
//! # Example (sync)
//!
//! ```rust,ignore
//! use gm65_scanner::{Gm65Scanner, ScannerDriverSync, ScannerConfig};
//!
//! let mut scanner = Gm65Scanner::new(uart, ScannerConfig::default());
//! scanner.init()?;
//! scanner.trigger_scan()?;
//! if let Some(data) = scanner.read_scan() {
//!     // use scanned data
//! }
//! ```
//!
//! # Example (async)
//!
//! ```rust,ignore
//! use gm65_scanner::{Gm65ScannerAsync, ScannerDriver, ScannerConfig};
//!
//! let mut scanner = Gm65ScannerAsync::new(uart, ScannerConfig::default());
//! scanner.init().await?;
//! scanner.trigger_scan().await?;
//! if let Some(data) = scanner.read_scan().await {
//!     // use scanned data
//! }
//! ```

#![no_std]

extern crate alloc;

pub mod buffer;
pub mod decoder;
pub mod driver;
pub mod hid_keyboard;
pub mod protocol;
pub mod scanner_core;

pub use buffer::ScanBuffer;
pub use decoder::{
    classify_payload, decode_payload, parse_ur_fragment, DecodedPayload, ParsedUrFragment,
    PayloadType, UrDecoder,
};
pub use driver::{
    DelayProvider, ScanMode, ScannerConfig, ScannerDriver, ScannerDriverSync, ScannerError,
    ScannerModel, ScannerState, ScannerStatus, SpinDelay,
};
pub use protocol::{
    build_factory_reset, build_get_setting, build_save_settings, build_set_setting,
    build_trigger_scan, commands, BaudRate as Gm65BaudRate, Gm65Response, Register, RESPONSE_LEN,
    RESPONSE_PREFIX,
};
pub use scanner_core::ScannerSettings;

// Re-export scanner core types
#[cfg(feature = "hil-tests")]
pub use scanner_core::HilTestResults;
pub use scanner_core::{InitStep, ScanByteResult, ScannerCore};

// Re-export sync scanner when sync feature is enabled
#[cfg(feature = "sync")]
pub use driver::Gm65Scanner;

// Re-export async scanner when async feature is enabled
#[cfg(feature = "async")]
pub use driver::Gm65ScannerAsync;
