//! Shared types for GM65 scanner drivers.

use core::fmt;

/// Scanner model identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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

/// Scanner scan mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ScanMode {
    Continuous,
    CommandTriggered,
    HardwareTriggered,
}

/// Scanner operational state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ScannerState {
    Uninitialized,
    Detecting,
    Configuring,
    Ready,
    Scanning,
    ScanComplete,
    Error(ScannerError),
}

/// Scanner error type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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

/// Scanner configuration
#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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

/// Scanner status snapshot
#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ScannerStatus {
    pub model: ScannerModel,
    pub connected: bool,
    pub initialized: bool,
    pub config: ScannerConfig,
    pub last_scan_len: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn test_scanner_model_display() {
        assert_eq!(format!("{}", ScannerModel::Gm65), "GM65");
        assert_eq!(format!("{}", ScannerModel::M3Y), "M3Y");
        assert_eq!(format!("{}", ScannerModel::Generic), "Generic");
        assert_eq!(format!("{}", ScannerModel::Unknown), "Unknown");
    }

    #[test]
    fn test_scanner_error_display() {
        assert_eq!(
            format!("{}", ScannerError::NotDetected),
            "Scanner not detected"
        );
        assert_eq!(
            format!("{}", ScannerError::Timeout),
            "Communication timeout"
        );
        assert_eq!(
            format!("{}", ScannerError::InvalidResponse),
            "Invalid response"
        );
        assert_eq!(
            format!("{}", ScannerError::BufferOverflow),
            "Buffer overflow"
        );
        assert_eq!(
            format!("{}", ScannerError::ConfigFailed),
            "Configuration failed"
        );
        assert_eq!(
            format!("{}", ScannerError::NotInitialized),
            "Not initialized"
        );
        assert_eq!(format!("{}", ScannerError::UartError), "UART error");
    }

    #[test]
    fn test_scanner_config_default() {
        let config = ScannerConfig::default();
        assert_eq!(config.model, ScannerModel::Unknown);
        assert_eq!(config.baud_rate, 9600);
        assert_eq!(config.mode, ScanMode::CommandTriggered);
        assert!(config.raw_mode);
    }

    #[test]
    fn test_scanner_status_fields() {
        let config = ScannerConfig::default();
        let status = ScannerStatus {
            model: ScannerModel::Gm65,
            connected: true,
            initialized: true,
            config: config.clone(),
            last_scan_len: Some(42),
        };
        assert_eq!(status.model, ScannerModel::Gm65);
        assert!(status.connected);
        assert!(status.initialized);
        assert_eq!(status.last_scan_len, Some(42));
    }
}
