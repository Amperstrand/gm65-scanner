//! GM65 Scanner Driver
//!
//! Hardware driver for GM65/M3Y QR scanner modules connected via UART.
//! Protocol modeled after specter-diy's qr.py:
//! https://github.com/cryptoadvance/specter-diy/blob/master/src/hosts/qr.py
//!
//! # Key Differences from the Datasheet
//!
//! The GM65 datasheet protocol description is incorrect/misleading. The real
//! protocol (as used by specter-diy, which ships working hardware) is:
//!
//! - Commands end with `AB CD` (sentinel, not a checksum)
//! - Responses are `02 00 00 01 [value] 33 31` (7 bytes, no `7E 00` header)
//! - Register addresses differ from datasheet (see protocol.rs)
//!
//! # Sync vs Async
//!
//! Two traits are provided:
//! - `ScannerDriverSync` — blocking, for polling main loops (no allocator needed for trait methods)
//! - `ScannerDriver` — async, for executor-based firmware (embassy, etc.)
//!
//! `Gm65Scanner<UART>` implements both when the `embedded-hal` feature is enabled.

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

pub const MAX_SCAN_SIZE: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerModel {
    Gm65,
    M3Y,
    Generic,
    Unknown,
}

impl fmt::Display for ScannerModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScannerModel::Gm65 => write!(f, "GM65"),
            ScannerModel::M3Y => write!(f, "M3Y"),
            ScannerModel::Generic => write!(f, "Generic"),
            ScannerModel::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    Continuous,
    CommandTriggered,
    HardwareTriggered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerState {
    Uninitialized,
    Detecting,
    Configuring,
    Ready,
    Scanning,
    ScanComplete,
    Error(ScannerError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerError {
    NotDetected,
    Timeout,
    InvalidResponse,
    BufferOverflow,
    ConfigFailed,
    NotInitialized,
    UartError,
}

impl fmt::Display for ScannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScannerError::NotDetected => write!(f, "Scanner not detected"),
            ScannerError::Timeout => write!(f, "Communication timeout"),
            ScannerError::InvalidResponse => write!(f, "Invalid response"),
            ScannerError::BufferOverflow => write!(f, "Buffer overflow"),
            ScannerError::ConfigFailed => write!(f, "Configuration failed"),
            ScannerError::NotInitialized => write!(f, "Not initialized"),
            ScannerError::UartError => write!(f, "UART error"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScannerConfig {
    pub model: ScannerModel,
    pub baud_rate: u32,
    pub mode: ScanMode,
    pub raw_mode: bool,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            model: ScannerModel::Unknown,
            baud_rate: 9600,
            mode: ScanMode::CommandTriggered,
            raw_mode: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScannerStatus {
    pub model: ScannerModel,
    pub connected: bool,
    pub initialized: bool,
    pub config: ScannerConfig,
    pub last_scan_len: Option<usize>,
}

pub trait ScannerDriverSync {
    fn init(&mut self) -> Result<ScannerModel, ScannerError>;
    fn ping(&mut self) -> bool;
    fn trigger_scan(&mut self) -> Result<(), ScannerError>;
    fn read_scan(&mut self) -> Option<Vec<u8>>;
    fn state(&self) -> ScannerState;
    fn status(&self) -> ScannerStatus;
    fn data_ready(&self) -> bool;
}

pub trait ScannerDriver {
    fn init(
        &mut self,
    ) -> impl core::future::Future<Output = Result<ScannerModel, ScannerError>> + Send;
    fn ping(&mut self) -> impl core::future::Future<Output = bool> + Send;
    fn trigger_scan(
        &mut self,
    ) -> impl core::future::Future<Output = Result<(), ScannerError>> + Send;
    fn read_scan(&mut self) -> impl core::future::Future<Output = Option<Vec<u8>>> + Send;
    fn state(&self) -> ScannerState;
    fn status(&self) -> ScannerStatus;
    fn data_ready(&self) -> bool;
}
