//! GM65 Scanner Protocol
//!
//! Command and response handling for GM65 QR scanner modules.
//! Protocol reverse-engineered from specter-diy's qr.py:
//! <https://github.com/cryptoadvance/specter-diy/blob/master/src/hosts/qr.py>
//!
//! # UART Transport
//!
//! Communication uses standard UART (8N1, no flow control) per EIA/TIA-232-F.
//! Supported baud rates: 9600, 19200, 38400, 57600, 115200 baud.
//!
//! # Protocol Format
//!
//! All commands follow this structure:
//! `[7E 00] [type:1] [len:1] [addr_hi] [addr_lo] [value:N] [AB CD]`
//!
//! - Header: `7E 00` (2 bytes)
//! - Type: `07` (get) or `08` (set) or `09` (save)
//! - Length: number of bytes following this field (addr + value)
//! - Address: 2-byte register address, MSB first (big-endian)
//! - Value: data bytes
//! - Suffix: `AB CD` (sentinel, NOT a real checksum)
//!
//! # Response Format
//!
//! Responses are 7 bytes: `02 00 00 01 [value_byte] 33 31`
//!
//! - Bytes 0-3: prefix `02 00 00 01` (success indicator)
//! - Byte 4: the register value (for get_setting responses)
//! - Bytes 5-6: `33 31` (constant suffix)
//!
//! **CRITICAL**: Responses do NOT start with `7E 00` and do NOT end with `0x55`.
//! The datasheet protocol description is misleading/incorrect.

extern crate alloc;

use alloc::vec::Vec;

/// Command frame header bytes.
pub const HEADER: [u8; 2] = [0x7E, 0x00];

/// Sentinel suffix appended to set/get commands (not a real CRC).
pub const CRC_NO_CHECKSUM: [u8; 2] = [0xAB, 0xCD];

/// Sentinel suffix for the save-settings command.
///
/// The GM65 save command uses `0xDE 0xC8` instead of the standard
/// `0xAB 0xCD` sentinel, confirmed by specter-diy's protocol implementation.
pub const SAVE_SENTINEL: [u8; 2] = [0xDE, 0xC8];

/// Response prefix: `02 00 00 01` indicates a successful operation.
pub const RESPONSE_PREFIX: [u8; 4] = [0x02, 0x00, 0x00, 0x01];

/// Expected response frame length in bytes.
pub const RESPONSE_LEN: usize = 7;

/// Command type: set register parameter.
pub const CMD_SET_PARAM: u8 = 0x08;

/// Command type: get register parameter.
pub const CMD_GET_PARAM: u8 = 0x07;

/// Command type: save settings to NVRAM.
pub const CMD_SAVE: u8 = 0x09;

/// GM65 register addresses.
///
/// Each variant's discriminant is the 16-bit register address used in
/// command frames. Use [`Register::address_bytes()`] to get the wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Register {
    /// Serial output mode (0x000D). Bits 0-1 control output format.
    SerialOutput = 0x000D,
    /// Settings register (0x0000). Controls always-on, sound, aim, light, mode.
    Settings = 0x0000,
    /// Baud rate (0x002A). 2-byte value register.
    BaudRate = 0x002A,
    /// Scan enable (0x0002). Write 0x01 to trigger, 0x00 to stop.
    ScanEnable = 0x0002,
    /// Inter-scan timeout (0x0006). 0x00 disables timeout.
    Timeout = 0x0006,
    /// Scan interval in milliseconds (0x0005).
    ScanInterval = 0x0005,
    /// Delay before re-scanning same barcode (0x0013).
    SameBarcodeDelay = 0x0013,
    /// Firmware version (0x00E2). Read-only.
    Version = 0x00E2,
    /// Raw mode setting (0x00BC). Some firmware versions require specific value.
    RawMode = 0x00BC,
    /// Barcode type filter (0x002C).
    ///
    /// **Known issue on GM65 firmware 0.87**: Write is ACKed but not persisted.
    /// Verify-read returns the original value. This is a firmware quirk — QR
    /// scanning works regardless since the scanner auto-detects barcode types.
    BarType = 0x002C,
    /// QR code enable (0x003F). Set to 0x01 to enable QR decoding.
    QrEnable = 0x003F,
    /// Factory reset (0x00D9). Write 0x55 to reset all settings.
    FactoryReset = 0x00D9,
}

impl Register {
    /// Return the 2-byte register address in wire format (MSB first).
    pub fn address_bytes(&self) -> [u8; 2] {
        let addr = *self as u16;
        [(addr >> 8) as u8, (addr & 0xFF) as u8]
    }
}

/// Supported baud rates for the GM65 UART interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BaudRate {
    Bps9600 = 0x00,
    Bps19200 = 0x01,
    Bps38400 = 0x02,
    Bps57600 = 0x03,
    Bps115200 = 0x1A,
}

impl BaudRate {
    /// Return the 1-byte baud rate code used in set-baud commands.
    pub fn value(&self) -> u8 {
        *self as u8
    }

    /// Return the actual baud rate as an integer (e.g., 115200).
    pub fn as_u32(&self) -> u32 {
        match self {
            BaudRate::Bps9600 => 9600,
            BaudRate::Bps19200 => 19200,
            BaudRate::Bps38400 => 38400,
            BaudRate::Bps57600 => 57600,
            BaudRate::Bps115200 => 115200,
        }
    }
}

/// Build a get-setting command frame (9 bytes).
///
/// Returns a complete command frame that reads the register at `addr`.
pub fn build_get_setting(addr: [u8; 2]) -> [u8; 9] {
    [
        HEADER[0],
        HEADER[1],
        CMD_GET_PARAM,
        0x01,
        addr[0],
        addr[1],
        0x01,
        CRC_NO_CHECKSUM[0],
        CRC_NO_CHECKSUM[1],
    ]
}

/// Build a set-setting command frame (9 bytes) with a 1-byte value.
pub fn build_set_setting(addr: [u8; 2], value: u8) -> [u8; 9] {
    [
        HEADER[0],
        HEADER[1],
        CMD_SET_PARAM,
        0x01,
        addr[0],
        addr[1],
        value,
        CRC_NO_CHECKSUM[0],
        CRC_NO_CHECKSUM[1],
    ]
}

/// Build a set-setting command frame (10 bytes) with a 2-byte value.
///
/// Used for registers that require 2-byte values (e.g., baud rate).
pub fn build_set_setting_2byte(addr: [u8; 2], value: [u8; 2]) -> [u8; 10] {
    [
        HEADER[0],
        HEADER[1],
        CMD_SET_PARAM,
        0x02,
        addr[0],
        addr[1],
        value[0],
        value[1],
        CRC_NO_CHECKSUM[0],
        CRC_NO_CHECKSUM[1],
    ]
}

/// Build a save-settings command frame (9 bytes).
///
/// Uses the save-specific sentinel `0xDE 0xC8` instead of the standard
/// `0xAB 0xCD` suffix. This is confirmed by the specter-diy protocol
/// implementation.
pub fn build_save_settings() -> [u8; 9] {
    [
        0x7E,
        0x00,
        0x09,
        0x01,
        0x00,
        0x00,
        0x00,
        SAVE_SENTINEL[0],
        SAVE_SENTINEL[1],
    ]
}

/// Build a factory-reset command frame (9 bytes).
pub fn build_factory_reset() -> [u8; 9] {
    [
        0x7E,
        0x00,
        0x08,
        0x01,
        0x00,
        0xD9,
        0x55,
        CRC_NO_CHECKSUM[0],
        CRC_NO_CHECKSUM[1],
    ]
}

/// Build a trigger-scan command frame (9 bytes).
///
/// Equivalent to `build_set_setting(Register::ScanEnable, 0x01)`.
pub fn build_trigger_scan() -> [u8; 9] {
    build_set_setting(Register::ScanEnable.address_bytes(), 0x01)
}

/// Parsed GM65 response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Gm65Response {
    /// Successful response carrying a register value byte.
    SuccessWithValue(u8),
    /// Successful response with no value (set/save operations).
    Success,
    /// Invalid or unrecognized response frame.
    Invalid,
}

impl Gm65Response {
    /// Parse a 7-byte response frame.
    ///
    /// Returns `SuccessWithValue(v)` if the prefix matches, where `v` is
    /// byte 4 of the response. Returns `Invalid` for wrong length or prefix.
    pub fn parse(data: &[u8]) -> Self {
        if data.len() != RESPONSE_LEN || data[0..4] != RESPONSE_PREFIX {
            return Gm65Response::Invalid;
        }
        Gm65Response::SuccessWithValue(data[4])
    }

    /// Parse a get-setting response. Alias for [`Self::parse()`].
    pub fn parse_get_response(data: &[u8]) -> Self {
        Self::parse(data)
    }

    /// Parse a set-setting response.
    ///
    /// Returns `Success` (no value) if the prefix matches, `Invalid` otherwise.
    /// Note: the actual driver code uses [`Self::parse()`] for all response types.
    pub fn parse_set_response(data: &[u8]) -> Self {
        if data.len() != RESPONSE_LEN || data[0..4] != RESPONSE_PREFIX {
            return Gm65Response::Invalid;
        }
        Gm65Response::Success
    }

    /// Returns `true` if the response indicates success.
    pub fn is_success(&self) -> bool {
        !matches!(self, Gm65Response::Invalid)
    }
}

/// Heap-allocated convenience wrappers around `build_*` functions.
///
/// **Note**: These allocate `Vec<u8>` on every call. In `no_std` embedded
/// code, prefer the stack-allocated `build_*` functions directly.
pub mod commands {
    use super::*;

    /// Build a factory-reset command (heap-allocated).
    pub fn factory_reset() -> Vec<u8> {
        build_factory_reset().to_vec()
    }

    /// Build a save-settings command (heap-allocated).
    pub fn save_settings() -> Vec<u8> {
        build_save_settings().to_vec()
    }

    /// Build an enable-serial-output command (heap-allocated).
    pub fn enable_serial_output() -> Vec<u8> {
        build_set_setting(Register::SerialOutput.address_bytes(), 0xA0).to_vec()
    }

    /// Build a set-baud-rate command (heap-allocated, 2-byte value).
    pub fn set_baud_rate(rate: BaudRate) -> Vec<u8> {
        build_set_setting_2byte(Register::BaudRate.address_bytes(), [rate.value(), 0x00]).to_vec()
    }

    /// Build an enable-raw-mode command (heap-allocated).
    pub fn enable_raw_mode() -> Vec<u8> {
        build_set_setting(Register::RawMode.address_bytes(), 0x08).to_vec()
    }

    /// Build a set-QR-only command (heap-allocated).
    pub fn set_qr_only() -> Vec<u8> {
        build_set_setting(Register::QrEnable.address_bytes(), 0x01).to_vec()
    }

    /// Build a trigger-scan command (heap-allocated).
    pub fn trigger_scan() -> Vec<u8> {
        build_trigger_scan().to_vec()
    }

    /// Build a get-setting command (heap-allocated).
    pub fn get_setting(addr: [u8; 2]) -> Vec<u8> {
        build_get_setting(addr).to_vec()
    }

    /// Build a set-setting command (heap-allocated, 1-byte value).
    pub fn set_setting(addr: [u8; 2], value: u8) -> Vec<u8> {
        build_set_setting(addr, value).to_vec()
    }

    /// Build a query-version command (heap-allocated).
    pub fn query_version() -> Vec<u8> {
        build_get_setting(Register::Version.address_bytes()).to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_setting_serial_addr() {
        let cmd = build_get_setting(Register::SerialOutput.address_bytes());
        assert_eq!(&cmd[..2], &[0x7E, 0x00]);
        assert_eq!(cmd[2], 0x07);
        assert_eq!(cmd[4], 0x00);
        assert_eq!(cmd[5], 0x0D);
        assert_eq!(&cmd[7..], &[0xAB, 0xCD]);
    }

    #[test]
    fn test_set_setting_baud_115200() {
        let cmd = build_set_setting_2byte(Register::BaudRate.address_bytes(), [0x1A, 0x00]);
        assert_eq!(&cmd[..2], &[0x7E, 0x00]);
        assert_eq!(cmd[2], 0x08);
        assert_eq!(cmd[3], 0x02);
        assert_eq!(&cmd[8..], &[0xAB, 0xCD]);
    }

    #[test]
    fn test_save_settings() {
        let cmd = build_save_settings();
        assert_eq!(&cmd[..2], &[0x7E, 0x00]);
        assert_eq!(cmd[2], 0x09);
        assert_eq!(&cmd[7..], &[0xDE, 0xC8]);
    }

    #[test]
    fn test_parse_success_response() {
        let resp = [0x02, 0x00, 0x00, 0x01, 0x87, 0x33, 0x31];
        let parsed = Gm65Response::parse_get_response(&resp);
        assert!(parsed.is_success());
        assert_eq!(parsed, Gm65Response::SuccessWithValue(0x87));
    }

    #[test]
    fn test_parse_invalid_response_wrong_len() {
        let resp = [0x02, 0x00, 0x00, 0x01];
        let parsed = Gm65Response::parse_get_response(&resp);
        assert_eq!(parsed, Gm65Response::Invalid);
    }

    #[test]
    fn test_register_addresses_match_specter_diy() {
        assert_eq!(Register::SerialOutput.address_bytes(), [0x00, 0x0D]);
        assert_eq!(Register::BaudRate.address_bytes(), [0x00, 0x2A]);
        assert_eq!(Register::RawMode.address_bytes(), [0x00, 0xBC]);
        assert_eq!(Register::FactoryReset.address_bytes(), [0x00, 0xD9]);
        assert_eq!(Register::Version.address_bytes(), [0x00, 0xE2]);
        assert_eq!(Register::ScanInterval.address_bytes(), [0x00, 0x05]);
        assert_eq!(Register::SameBarcodeDelay.address_bytes(), [0x00, 0x13]);
        assert_eq!(Register::BarType.address_bytes(), [0x00, 0x2C]);
        assert_eq!(Register::QrEnable.address_bytes(), [0x00, 0x3F]);
    }

    #[test]
    fn test_register_addresses_all() {
        assert_eq!(Register::Settings.address_bytes(), [0x00, 0x00]);
        assert_eq!(Register::ScanEnable.address_bytes(), [0x00, 0x02]);
        assert_eq!(Register::Timeout.address_bytes(), [0x00, 0x06]);
        assert_eq!(Register::ScanInterval.address_bytes(), [0x00, 0x05]);
        assert_eq!(Register::SameBarcodeDelay.address_bytes(), [0x00, 0x13]);
        assert_eq!(Register::SerialOutput.address_bytes(), [0x00, 0x0D]);
        assert_eq!(Register::BaudRate.address_bytes(), [0x00, 0x2A]);
        assert_eq!(Register::BarType.address_bytes(), [0x00, 0x2C]);
        assert_eq!(Register::QrEnable.address_bytes(), [0x00, 0x3F]);
        assert_eq!(Register::RawMode.address_bytes(), [0x00, 0xBC]);
        assert_eq!(Register::FactoryReset.address_bytes(), [0x00, 0xD9]);
        assert_eq!(Register::Version.address_bytes(), [0x00, 0xE2]);
    }

    #[test]
    fn test_build_set_setting() {
        let cmd = build_set_setting(Register::Settings.address_bytes(), 0x81);
        assert_eq!(&cmd[..2], &[0x7E, 0x00]);
        assert_eq!(cmd[2], 0x08);
        assert_eq!(cmd[3], 0x01);
        assert_eq!(cmd[6], 0x81);
        assert_eq!(&cmd[7..], &[0xAB, 0xCD]);
    }

    #[test]
    fn test_build_factory_reset() {
        let cmd = build_factory_reset();
        assert_eq!(&cmd[..2], &[0x7E, 0x00]);
        assert_eq!(cmd[2], 0x08);
        assert_eq!(cmd[4], 0x00);
        assert_eq!(cmd[5], 0xD9);
        assert_eq!(cmd[6], 0x55);
        assert_eq!(&cmd[7..], &[0xAB, 0xCD]);
    }

    #[test]
    fn test_build_trigger_scan() {
        let cmd = build_trigger_scan();
        assert_eq!(&cmd[..2], &[0x7E, 0x00]);
        assert_eq!(cmd[2], 0x08);
        assert_eq!(cmd[4..6], Register::ScanEnable.address_bytes());
        assert_eq!(cmd[6], 0x01);
        assert_eq!(&cmd[7..], &[0xAB, 0xCD]);
    }

    #[test]
    fn test_parse_set_response() {
        let resp = [0x02, 0x00, 0x00, 0x01, 0x00, 0x33, 0x31];
        let parsed = Gm65Response::parse_set_response(&resp);
        assert!(parsed.is_success());
        assert_eq!(parsed, Gm65Response::Success);
    }

    #[test]
    fn test_parse_set_response_invalid() {
        let resp = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let parsed = Gm65Response::parse_set_response(&resp);
        assert_eq!(parsed, Gm65Response::Invalid);
    }

    #[test]
    fn test_is_success() {
        assert!(Gm65Response::SuccessWithValue(0x87).is_success());
        assert!(Gm65Response::Success.is_success());
        assert!(!Gm65Response::Invalid.is_success());
    }

    #[test]
    fn test_baud_rate_value() {
        assert_eq!(BaudRate::Bps9600.value(), 0x00);
        assert_eq!(BaudRate::Bps19200.value(), 0x01);
        assert_eq!(BaudRate::Bps38400.value(), 0x02);
        assert_eq!(BaudRate::Bps57600.value(), 0x03);
        assert_eq!(BaudRate::Bps115200.value(), 0x1A);
    }

    #[test]
    fn test_baud_rate_as_u32() {
        assert_eq!(BaudRate::Bps9600.as_u32(), 9600);
        assert_eq!(BaudRate::Bps19200.as_u32(), 19200);
        assert_eq!(BaudRate::Bps38400.as_u32(), 38400);
        assert_eq!(BaudRate::Bps57600.as_u32(), 57600);
        assert_eq!(BaudRate::Bps115200.as_u32(), 115200);
    }

    #[test]
    fn test_commands_convenience() {
        let trigger = commands::trigger_scan();
        assert_eq!(trigger.as_slice(), &build_trigger_scan());

        let save = commands::save_settings();
        assert_eq!(save.as_slice(), &build_save_settings());

        let reset = commands::factory_reset();
        assert_eq!(reset.as_slice(), &build_factory_reset());

        let version = commands::query_version();
        assert_eq!(
            version.as_slice(),
            &build_get_setting(Register::Version.address_bytes())
        );

        let qr = commands::set_qr_only();
        assert_eq!(
            qr.as_slice(),
            &build_set_setting(Register::QrEnable.address_bytes(), 0x01)
        );

        let serial = commands::enable_serial_output();
        assert_eq!(
            serial.as_slice(),
            &build_set_setting(Register::SerialOutput.address_bytes(), 0xA0)
        );

        let baud = commands::set_baud_rate(BaudRate::Bps115200);
        assert_eq!(
            baud.as_slice(),
            &build_set_setting_2byte(Register::BaudRate.address_bytes(), [0x1A, 0x00])
        );

        let raw = commands::enable_raw_mode();
        assert_eq!(
            raw.as_slice(),
            &build_set_setting(Register::RawMode.address_bytes(), 0x08)
        );
    }
}
