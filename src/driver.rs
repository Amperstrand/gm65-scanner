//! GM65 Scanner Driver
//!
//! Hardware driver for GM65/M3Y QR scanner modules.

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

use crate::buffer::ScanBuffer;
use crate::protocol::{HEADER, FOOTER, CMD_SET_PARAM, CMD_GET_PARAM, Register, BaudRate, calculate_crc};

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
    Uninitialized
    Detecting
    Configuring
    Ready
    Scanning
    ScanComplete
    Error(ScannerError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerError {
    NotDetected
    Timeout
    InvalidResponse
    BufferOverflow
    ConfigFailed
    NotInitialized
    UartError,
}

impl fmt::Display for ScannerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

pub trait ScannerDriver {
    async fn init(&mut self) -> Result<ScannerModel, ScannerError>;
    
    async fn ping(&mut self) -> bool;
    
    async fn trigger_scan(&mut self) -> Result<(), ScannerError>;
    
    async fn read_scan(&mut self) -> Option<Vec<u8>>;
    
    fn state(&self) -> ScannerState;
    
    fn status(&self) -> ScannerStatus;
    
    fn data_ready(&self) -> bool;
}

