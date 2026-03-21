//! GM65 Scanner Protocol
//!
//! Command and response handling for GM65 QR scanner modules.
//! Protocol reverse-engineered from specter-diy's qr.py:
//! https://github.com/cryptoadvance/specter-diy/blob/master/src/hosts/qr.py
//!
//! # Protocol Format
//!
//! All commands follow this structure:
//! `[7E 00] [type:1] [len:1] [addr_lo] [addr_hi] [value:N] [AB CD]`
//!
//! - Header: `7E 00` (2 bytes)
//! - Type: `07` (get) or `08` (set) or `09` (save)
//! - Length: number of bytes following this field (addr + value)
//! - Address: 2-byte register address (little-endian from specter-diy)
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

pub const HEADER: [u8; 2] = [0x7E, 0x00];
pub const CRC_NO_CHECKSUM: [u8; 2] = [0xAB, 0xCD];

pub const RESPONSE_PREFIX: [u8; 4] = [0x02, 0x00, 0x00, 0x01];
pub const RESPONSE_LEN: usize = 7;

pub const CMD_SET_PARAM: u8 = 0x08;
pub const CMD_GET_PARAM: u8 = 0x07;
pub const CMD_SAVE: u8 = 0x09;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Register {
    SerialOutput = 0x000D,
    Settings = 0x0000,
    BaudRate = 0x002A,
    ScanEnable = 0x0002,
    Timeout = 0x0006,
    ScanInterval = 0x0005,
    SameBarcodeDelay = 0x0013,
    Version = 0x00E2,
    RawMode = 0x00BC,
    BarType = 0x002C,
    QrEnable = 0x003F,
    FactoryReset = 0x00D9,
}

impl Register {
    pub fn address_bytes(&self) -> [u8; 2] {
        let addr = *self as u16;
        [(addr >> 8) as u8, (addr & 0xFF) as u8]
    }
}

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
    pub fn value(&self) -> u8 {
        *self as u8
    }

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

pub fn build_save_settings() -> [u8; 9] {
    [0x7E, 0x00, 0x09, 0x01, 0x00, 0x00, 0x00, 0xDE, 0xC8]
}

pub fn build_factory_reset() -> [u8; 9] {
    [0x7E, 0x00, 0x08, 0x01, 0x00, 0xD9, 0x55, 0xAB, 0xCD]
}

pub fn build_trigger_scan() -> [u8; 9] {
    build_set_setting(Register::ScanEnable.address_bytes(), 0x01)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gm65Response {
    SuccessWithValue(u8),
    Success,
    Invalid,
}

impl Gm65Response {
    pub fn parse(data: &[u8]) -> Self {
        if data.len() != RESPONSE_LEN || data[0..4] != RESPONSE_PREFIX {
            return Gm65Response::Invalid;
        }
        Gm65Response::SuccessWithValue(data[4])
    }

    pub fn parse_get_response(data: &[u8]) -> Self {
        Self::parse(data)
    }

    pub fn parse_set_response(data: &[u8]) -> Self {
        if data.len() != RESPONSE_LEN || data[0..4] != RESPONSE_PREFIX {
            return Gm65Response::Invalid;
        }
        Gm65Response::Success
    }

    pub fn is_success(&self) -> bool {
        !matches!(self, Gm65Response::Invalid)
    }
}

pub mod commands {
    use super::*;

    pub fn factory_reset() -> Vec<u8> {
        build_factory_reset().to_vec()
    }

    pub fn save_settings() -> Vec<u8> {
        build_save_settings().to_vec()
    }

    pub fn enable_serial_output() -> Vec<u8> {
        build_set_setting(Register::SerialOutput.address_bytes(), 0xA0).to_vec()
    }

    pub fn set_baud_rate(rate: BaudRate) -> Vec<u8> {
        build_set_setting_2byte(Register::BaudRate.address_bytes(), [rate.value(), 0x00]).to_vec()
    }

    pub fn enable_raw_mode() -> Vec<u8> {
        build_set_setting(Register::RawMode.address_bytes(), 0x08).to_vec()
    }

    pub fn set_qr_only() -> Vec<u8> {
        build_set_setting(Register::QrEnable.address_bytes(), 0x01).to_vec()
    }

    pub fn trigger_scan() -> Vec<u8> {
        build_trigger_scan().to_vec()
    }

    pub fn get_setting(addr: [u8; 2]) -> Vec<u8> {
        build_get_setting(addr).to_vec()
    }

    pub fn set_setting(addr: [u8; 2], value: u8) -> Vec<u8> {
        build_set_setting(addr, value).to_vec()
    }

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
}
