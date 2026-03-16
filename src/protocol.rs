//! GM65/M3Y Scanner Protocol
//!
//! Low-level command and response handling for GM65 scanner modules.

extern crate alloc;

use alloc::vec::Vec;

pub const HEADER: [u8; 2] = [0x7E, 0x00];
pub const FOOTER: u8 = 0x55;

pub const CMD_SET_PARAM: u8 = 0x08;
pub const CMD_GET_PARAM: u8 = 0x07;
pub const CMD_QUERY_VERSION: u8 = 0x01;
pub const CMD_TRIGGER_SCAN: u8 = 0x04;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register {
    SerialOutput = 0x0000,
    BaudRate = 0x002A,
    RawMode = 0x00BC,
    FactoryReset = 0x00D9,
    ScanMode = 0x0001,
    QrOnly = 0x0002,
    ScanInterval = 0x0003,
}

impl Register {
    pub fn address_bytes(&self) -> [u8; 2] {
        let addr = *self as u16;
        [(addr >> 8) as u8, (addr & 0xFF) as u8]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

pub fn calculate_crc(data: &[u8]) -> u8 {
    data.iter().fold(0, |acc, &b| acc ^ b)
}

pub struct Gm65CommandBuilder {
    cmd_type: u8,
    register: Register,
    value: Vec<u8>,
}

impl Gm65CommandBuilder {
    pub fn set(register: Register) -> Self {
        Self {
            cmd_type: CMD_SET_PARAM,
            register,
            value: Vec::new(),
        }
    }

    pub fn get(register: Register) -> Self {
        Self {
            cmd_type: CMD_GET_PARAM,
            register,
            value: Vec::new(),
        }
    }

    pub fn with_value(mut self, value: u8) -> Self {
        self.value.push(value);
        self
    }

    pub fn with_values(mut self, values: &[u8]) -> Self {
        self.value.extend_from_slice(values);
        self
    }

    pub fn build(self) -> Vec<u8> {
        let addr = self.register.address_bytes();
        let payload_len = 2 + self.value.len();

        let mut cmd = Vec::with_capacity(8 + self.value.len());
        cmd.extend_from_slice(&HEADER);
        cmd.push(self.cmd_type);
        cmd.push(payload_len as u8);
        cmd.extend_from_slice(&addr);
        cmd.extend_from_slice(&self.value);

        let crc = calculate_crc(&cmd[2..]);
        cmd.push(crc);
        cmd.push(FOOTER);

        cmd
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gm65Response {
    Ack,
    Nack(u8),
    Version { major: u8, minor: u8 },
    ParamValue(Vec<u8>),
    Invalid,
}

impl Gm65Response {
    pub fn parse(data: &[u8]) -> Self {
        if data.len() < 6 {
            return Gm65Response::Invalid;
        }

        if data[0] != HEADER[0] || data[1] != HEADER[1] {
            return Gm65Response::Invalid;
        }

        if data[data.len() - 1] != FOOTER {
            return Gm65Response::Invalid;
        }

        let expected_crc = calculate_crc(&data[2..data.len() - 2]);
        if data[data.len() - 2] != expected_crc {
            return Gm65Response::Invalid;
        }

        let status = data[3];
        match status {
            0x00 => Gm65Response::Ack,
            0xEE => Gm65Response::Nack(data.get(4).copied().unwrap_or(0)),
            _ => {
                if data.len() > 5 {
                    Gm65Response::ParamValue(data[4..data.len() - 2].to_vec())
                } else {
                    Gm65Response::Invalid
                }
            }
        }
    }
}

pub mod commands {
    use super::*;
    use alloc::vec::Vec;

    pub fn factory_reset() -> Vec<u8> {
        Gm65CommandBuilder::set(Register::FactoryReset)
            .with_value(0x00)
            .build()
    }

    pub fn enable_serial_output() -> Vec<u8> {
        Gm65CommandBuilder::set(Register::SerialOutput)
            .with_value(0x01)
            .build()
    }

    pub fn set_baud_rate(rate: BaudRate) -> Vec<u8> {
        Gm65CommandBuilder::set(Register::BaudRate)
            .with_value(rate.value())
            .build()
    }

    pub fn enable_raw_mode() -> Vec<u8> {
        Gm65CommandBuilder::set(Register::RawMode)
            .with_value(0x08)
            .build()
    }

    pub fn set_qr_only() -> Vec<u8> {
        Gm65CommandBuilder::set(Register::QrOnly)
            .with_value(0x01)
            .build()
    }

    pub fn query_version() -> Vec<u8> {
        alloc::vec![0x7E, 0x00, 0x01, 0x00, 0x01, 0x01, 0x55]
    }

    pub fn trigger_scan() -> Vec<u8> {
        alloc::vec![0x7E, 0x00, 0x04, 0x00, 0x04, 0x00, 0x55]
    }
}
