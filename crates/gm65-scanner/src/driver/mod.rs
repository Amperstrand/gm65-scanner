//! GM65 Scanner Driver Module
//!
//! Provides both sync (blocking) and async driver implementations for GM65/M3Y
//! QR barcode scanner modules.
//!
//! # Feature Flags
//!
//! - `sync` (default) — Enables `Gm65Scanner<UART>` with blocking I/O
//! - `async` — Enables `Gm65ScannerAsync<UART>` with async I/O
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
//!     // process scanned data
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
//!     // process scanned data
//! }
//! ```

mod traits;
mod types;

pub use traits::{ScannerDriver, ScannerDriverSync};
pub use types::{ScanMode, ScannerConfig, ScannerError, ScannerModel, ScannerState, ScannerStatus};

#[cfg(feature = "sync")]
mod sync;

#[cfg(feature = "sync")]
pub use sync::Gm65Scanner;

#[cfg(all(feature = "sync", feature = "hil-tests"))]
pub use sync::hil_tests;

#[cfg(feature = "async")]
mod async_;

#[cfg(feature = "async")]
pub use async_::Gm65ScannerAsync;

#[cfg(all(feature = "async", feature = "hil-tests"))]
pub use async_::hil_tests as async_hil_tests;

#[cfg(all(feature = "async", feature = "hil-tests"))]
pub use async_::run_extended_hil_tests;
