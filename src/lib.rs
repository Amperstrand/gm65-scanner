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
//! - `embedded-hal` - Enable `Gm65Scanner<UART>` with `ScannerDriverSync` trait impl
//! - `embedded-hal-async` - Also enable async `ScannerDriver` trait impl
//! - `std` - Enable standard library support
//!
//! # Sync vs Async
//!
//! Two traits are provided for scanner communication:
//!
//! - `ScannerDriverSync` — blocking methods, for polling main loops
//! - `ScannerDriver` — async methods, for executor-based firmware
//!
//! `Gm65Scanner<UART>` implements both when the respective features are enabled.
//!
//! # Example (sync)
//!
//! ```rust,ignore
//! use gm65_scanner::{Gm65Scanner, ScannerDriverSync, ScannerConfig};
//!
//! let mut scanner = Gm65Scanner::with_default_config(uart);
//! scanner.init().ok();
//! scanner.trigger_scan().ok();
//! if let Some(data) = scanner.read_scan() {
//!     // use scanned data
//! }
//! ```

#![no_std]
#![doc(html_root_url = "https://docs.rs/gm65-scanner/")]

extern crate alloc;

pub mod buffer;
pub mod decoder;
pub mod driver;
pub mod protocol;
#[cfg(feature = "embedded-hal")]
pub mod scanner;

pub use buffer::ScanBuffer;
pub use decoder::{classify_payload, decode_payload, DecodedPayload, PayloadType};
pub use driver::{
    ScanMode, ScannerConfig, ScannerDriver, ScannerDriverSync, ScannerError, ScannerModel,
    ScannerState, ScannerStatus,
};
pub use protocol::{
    build_factory_reset, build_get_setting, build_save_settings, build_set_setting,
    build_trigger_scan, commands, BaudRate as Gm65BaudRate, Gm65Response, Register,
};
#[cfg(feature = "embedded-hal")]
pub use scanner::Gm65Scanner;
