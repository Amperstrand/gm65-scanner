//! Driver traits for GM65 scanner.
//!
//! Provides both sync (blocking) and async driver traits.

use super::types::{ScannerError, ScannerModel, ScannerState, ScannerStatus};

/// Blocking (synchronous) scanner driver trait.
///
/// Implement this trait for polling-based main loops that don't use
/// an async executor. All methods are blocking.
pub trait ScannerDriverSync {
    /// Initialize the scanner and detect model.
    /// Returns the detected model on success, or an error.
    fn init(&mut self) -> Result<ScannerModel, ScannerError>;

    /// Ping the scanner to check communication.
    /// Returns true if the scanner responds.
    fn ping(&mut self) -> bool;

    /// Trigger a scan.
    /// Returns Ok(()) if the trigger command was sent successfully.
    fn trigger_scan(&mut self) -> Result<(), ScannerError>;

    /// Stop an ongoing scan.
    /// Returns true if the stop command was successful.
    fn stop_scan(&mut self) -> bool;

    /// Read scanned data (blocking).
    /// Returns Some(data) if a complete scan was read, None on timeout.
    fn read_scan(&mut self) -> Option<alloc::vec::Vec<u8>>;

    /// Get the current scanner state.
    fn state(&self) -> ScannerState;

    /// Get the scanner status.
    fn status(&self) -> ScannerStatus;

    /// Check if scanned data is ready.
    fn data_ready(&self) -> bool;
}

/// Async scanner driver trait for executor-based firmware (embassy, etc.).
/// Futures are `?Send` for single-threaded embedded executors.
pub trait ScannerDriver {
    /// Initialize the scanner and detect model.
    /// Returns the detected model on success, or an error.
    fn init(&mut self) -> impl core::future::Future<Output = Result<ScannerModel, ScannerError>>;

    /// Ping the scanner to check communication.
    /// Returns true if the scanner responds.
    fn ping(&mut self) -> impl core::future::Future<Output = bool>;

    /// Trigger a scan.
    /// Returns Ok(()) if the trigger command was sent successfully.
    fn trigger_scan(&mut self) -> impl core::future::Future<Output = Result<(), ScannerError>>;

    /// Stop an ongoing scan.
    /// Returns true if the stop command was successful.
    fn stop_scan(&mut self) -> impl core::future::Future<Output = bool>;

    /// Read scanned data (async).
    /// Returns Some(data) if a complete scan was read, None on timeout.
    fn read_scan(&mut self) -> impl core::future::Future<Output = Option<alloc::vec::Vec<u8>>>;

    /// Get the current scanner state.
    fn state(&self) -> ScannerState;

    /// Get the scanner status.
    fn status(&self) -> ScannerStatus;

    /// Check if scanned data is ready.
    fn data_ready(&self) -> bool;
}
