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
//! - `embedded-hal` - Enable embedded-hal trait implementations
//! - `std` - Enable standard library support
//!
//! # Example
//!
//! ```rust,ignore
//! use gm65_scanner::{Gm65Scanner, ScannerConfig, ScanMode};
//!
//! let config = ScannerConfig {
//!     baud_rate: 115200,
//!     mode: ScanMode::CommandTriggered,
//!     raw_mode: true,
//!     ..Default::default()
//! };
//!
//! let mut scanner = Gm65Scanner::new(uart, config);
//! ```

#![no_std]
#![doc(html_root_url = "https://docs.rs/gm65-scanner/")]

extern crate alloc;

pub mod buffer;
pub mod decoder;
pub mod driver;
pub mod protocol;

pub use buffer::ScanBuffer;
pub use decoder::{classify_payload, decode_payload, DecodedPayload, PayloadType};
pub use driver::{
    ScanMode, ScannerConfig, ScannerDriver, ScannerError, ScannerModel, ScannerState, ScannerStatus,
};
pub use protocol::{
    build_factory_reset, build_get_setting, build_save_settings, build_set_setting,
    build_trigger_scan, commands, BaudRate as Gm65BaudRate, Gm65Response, Register,
};
