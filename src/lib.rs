//! GM65/M3Y QR Scanner Driver
//!
//! A no_std compatible driver for GM65 and M3Y QR barcode scanner modules.
//! These scanners communicate via UART and handle QR decoding internally.
//!
//! # Features
//!
//! - `embedded-hal` - Enable embedded-hal trait implementations
//! - `embedded-hal-async` - Enable async embedded-hal support
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
//! let mut scanner = Gm65Scanner::new(uart, Some(trigger_pin), config);
//! ```

#![no_std]
#![doc(html_root_url = "https://docs.rs/gm65-scanner/")]

extern crate alloc;

pub mod buffer;
pub mod decoder;
pub mod driver;
pub mod protocol;

pub use buffer::ScanBuffer;
pub use decoder::{decode_payload, classify_payload, PayloadType};
pub use driver::{ScanMode, ScannerConfig, ScannerError, ScannerModel, ScannerState, ScannerDriver};
pub use protocol::{calculate_crc, commands, BaudRate as Gm65BaudRate, Gm65CommandBuilder, Gm65Response, Register};
