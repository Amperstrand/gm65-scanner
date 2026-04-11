//! Shared types for GM65 scanner drivers.

use core::fmt;

/// Scanner model identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[must_use = "ignoring detected scanner model is likely a bug"]
pub enum ScannerModel {
    /// GM65 scanner module.
    Gm65,
    /// M3Y scanner module (GM65 variant).
    M3Y,
    /// Unrecognized model (init succeeded but version unknown).
    Generic,
    /// Model not yet detected (pre-init state).
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

/// Scanner scan mode configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ScanMode {
    /// Scanner triggers continuously while barcode is in view.
    Continuous,
    /// Scanner waits for a trigger command before scanning.
    CommandTriggered,
    /// Scanner uses a hardware trigger pin.
    HardwareTriggered,
}

/// Scanner operational state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ScannerState {
    /// Scanner has not been initialized.
    Uninitialized,
    /// Probe in progress (drain + ping).
    Detecting,
    /// Writing configuration registers.
    Configuring,
    /// Scanner is idle and ready to accept scan commands.
    Ready,
    /// A scan is in progress (waiting for barcode data).
    Scanning,
    /// Scan data received and available via `read_scan()`.
    ScanComplete,
    /// An error occurred (contains the specific error).
    Error(ScannerError),
}

/// Scanner error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[must_use = "ignoring a scanner error is likely a bug"]
pub enum ScannerError {
    /// Scanner did not respond to probe ping.
    NotDetected,
    /// UART communication timed out.
    Timeout,
    /// Response frame was invalid or unrecognized.
    InvalidResponse,
    /// Scan data exceeded buffer capacity.
    BufferOverflow,
    /// A configuration register write or read failed.
    ConfigFailed,
    /// Operation attempted before calling `init()`.
    NotInitialized,
    /// UART read/write error.
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

impl core::error::Error for ScannerError {}

/// Scanner configuration.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[must_use = "ignoring scanner configuration is likely a bug"]
pub struct ScannerConfig {
    /// Detected or expected scanner model.
    pub model: ScannerModel,
    /// UART baud rate (informational — driver does not set this).
    pub baud_rate: u32,
    /// Scan triggering mode.
    pub mode: ScanMode,
    /// Whether raw mode is enabled (firmware-specific).
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

/// Scanner status snapshot.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[must_use = "ignoring scanner status is likely a bug"]
pub struct ScannerStatus {
    /// Detected scanner model.
    pub model: ScannerModel,
    /// `true` if the scanner has been initialized successfully.
    pub connected: bool,
    /// Alias for `connected` — `true` after successful `init()`.
    pub initialized: bool,
    /// Configuration used during initialization.
    pub config: ScannerConfig,
    /// Byte length of the most recent scan, if any.
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
            config,
            last_scan_len: Some(42),
        };
        assert_eq!(status.model, ScannerModel::Gm65);
        assert!(status.connected);
        assert!(status.initialized);
        assert_eq!(status.last_scan_len, Some(42));
    }
}
