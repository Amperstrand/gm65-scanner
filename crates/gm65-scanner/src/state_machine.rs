//! GM65 Scanner State Machine
//!
//! Transport-agnostic state machine for GM65 scanner configuration and operation.
//! This module defines the configuration sequence and state transitions without
//! any UART I/O dependencies.

use crate::protocol::Register;

/// Configuration constants for GM65 scanner
pub mod config {
    /// Scan interval in milliseconds
    pub const SCAN_INTERVAL_MS: u8 = 0x01;
    /// Delay before scanning same barcode again
    pub const SAME_BARCODE_DELAY: u8 = 0x85;
    /// Command mode settings value (ALWAYS_ON | SOUND | AIM | COMMAND)
    pub const CMD_MODE: u8 = 0xD1;
    /// Firmware version that requires raw mode fix
    pub const VERSION_NEEDS_RAW: u8 = 0x69;
    /// Raw mode value for firmware fix
    pub const RAW_MODE_VALUE: u8 = 0x08;
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ScannerSettings: u8 {
        const ALWAYS_ON  = 1 << 7;
        const SOUND      = 1 << 6;
        const UNKNOWN_5  = 1 << 5;
        const AIM        = 1 << 4;
        const UNKNOWN_3  = 1 << 3;
        const LIGHT      = 1 << 2;
        const CONTINUOUS = 1 << 1;
        const COMMAND    = 1 << 0;
    }
}

impl Default for ScannerSettings {
    fn default() -> Self {
        Self::ALWAYS_ON | Self::SOUND | Self::AIM | Self::COMMAND
    }
}

/// Register configuration tuple: (register, expected_value)
pub type RegisterConfig = (Register, u8);

/// Returns the standard initialization register configuration sequence.
/// Each tuple contains a register and its target value.
pub fn init_config_sequence() -> [RegisterConfig; 5] {
    use config::*;
    [
        (Register::Timeout, 0x00),
        (Register::ScanInterval, SCAN_INTERVAL_MS),
        (Register::SameBarcodeDelay, SAME_BARCODE_DELAY),
        (Register::BarType, 0x01),
        (Register::QrEnable, 0x01),
    ]
}

/// Returns registers that should be checked during init for special handling.
pub fn special_registers() -> [Register; 3] {
    [
        Register::SerialOutput, // Needs bits 0-1 cleared
        Register::Settings,     // Needs to be CMD_MODE
        Register::Version,      // May need raw mode fix
    ]
}

/// Check if SerialOutput value needs fixing (bits 0-1 should be 0).
#[inline]
pub fn serial_output_needs_fix(value: u8) -> bool {
    value & 0x03 != 0
}

/// Fix SerialOutput value by clearing bits 0-1.
#[inline]
pub fn fix_serial_output(value: u8) -> u8 {
    value & 0xFC
}

/// Check if firmware version needs raw mode fix.
#[inline]
pub fn version_needs_raw_fix(version: u8) -> bool {
    version == config::VERSION_NEEDS_RAW
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_settings_default() {
        let settings = ScannerSettings::default();
        assert!(settings.contains(ScannerSettings::ALWAYS_ON));
        assert!(settings.contains(ScannerSettings::SOUND));
        assert!(settings.contains(ScannerSettings::AIM));
        assert!(settings.contains(ScannerSettings::COMMAND));
        assert!(!settings.contains(ScannerSettings::CONTINUOUS));
    }

    #[test]
    fn test_serial_output_fix() {
        assert!(serial_output_needs_fix(0xA3));
        assert!(!serial_output_needs_fix(0xA0));
        assert_eq!(fix_serial_output(0xA3), 0xA0);
        assert_eq!(fix_serial_output(0xA0), 0xA0);
    }

    #[test]
    fn test_version_needs_raw_fix() {
        assert!(version_needs_raw_fix(0x69));
        assert!(!version_needs_raw_fix(0x87));
    }

    #[test]
    fn test_init_config_sequence_length() {
        let seq = init_config_sequence();
        assert_eq!(seq.len(), 5);
    }
}
