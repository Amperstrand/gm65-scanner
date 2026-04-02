// CDC Application-Level Protocol
//
// This module implements a custom command/response framing protocol
// layered on top of USB CDC ACM (per USB CDC Specification 1.2).
//
// The CDC ACM transport provides a virtual serial port; this protocol
// adds structured framing for scanner control commands.
//
// Frame format (request):  [command:1][length_hi:1][length_lo:1][payload:N]
// Frame format (response): [status:1][length_hi:1][length_lo:1][payload:N]
//
// This is a proprietary application protocol, not part of any USB standard.
// For standards-based alternatives, see:
// - HID keyboard wedge (USB HID Usage Tables 1.5, §10)
// - HID POS barcode scanner (USB-IF HID POS Usage Tables 1.02)

#[cfg(not(feature = "scanner-async"))]
use stm32f469i_disc::hal::otg_fs::UsbBusType;

pub const MAX_PAYLOAD_SIZE: usize = 256;

#[cfg(not(feature = "scanner-async"))]
const RX_BUF_SIZE: usize = 3 + MAX_PAYLOAD_SIZE;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    ScannerStatus = 0x10,
    ScannerTrigger = 0x11,
    ScannerData = 0x12,
    GetSettings = 0x13,
    SetSettings = 0x14,
    DisplayQr = 0x15,
    EnterSettings = 0x16,
}

impl Command {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x10 => Some(Command::ScannerStatus),
            0x11 => Some(Command::ScannerTrigger),
            0x12 => Some(Command::ScannerData),
            0x13 => Some(Command::GetSettings),
            0x14 => Some(Command::SetSettings),
            0x15 => Some(Command::DisplayQr),
            0x16 => Some(Command::EnterSettings),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Status {
    Ok = 0x00,
    Error = 0xFF,
    InvalidCommand = 0x01,
    InvalidPayload = 0x02,
    BufferOverflow = 0x03,
    ScannerNotConnected = 0x10,
    ScannerBusy = 0x11,
    NoScanData = 0x12,
}

impl Status {
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub command: Command,
    pub length: u16,
    pub payload: [u8; MAX_PAYLOAD_SIZE],
}

impl Frame {
    pub fn new(command: Command) -> Self {
        Self {
            command,
            length: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }

    pub fn with_payload(command: Command, data: &[u8]) -> Option<Self> {
        if data.len() > MAX_PAYLOAD_SIZE {
            return None;
        }
        let mut frame = Self::new(command);
        frame.length = data.len() as u16;
        frame.payload[..data.len()].copy_from_slice(data);
        Some(frame)
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.length as usize]
    }
}

#[cfg(not(feature = "scanner-async"))]
#[derive(Debug, Clone)]
pub struct Response {
    pub status: Status,
    pub length: u16,
    pub payload: [u8; MAX_PAYLOAD_SIZE],
}

#[cfg(not(feature = "scanner-async"))]
impl Response {
    pub fn new(status: Status) -> Self {
        Self {
            status,
            length: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }

    pub fn with_payload(status: Status, data: &[u8]) -> Option<Self> {
        if data.len() > MAX_PAYLOAD_SIZE {
            return None;
        }
        let mut resp = Self::new(status);
        resp.length = data.len() as u16;
        resp.payload[..data.len()].copy_from_slice(data);
        Some(resp)
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.length as usize]
    }

    pub fn encode(&self, buf: &mut [u8]) -> usize {
        let total_len = 3 + self.length as usize;
        if buf.len() < total_len {
            return 0;
        }
        buf[0] = self.status.to_byte();
        buf[1] = (self.length >> 8) as u8;
        buf[2] = (self.length & 0xFF) as u8;
        buf[3..total_len].copy_from_slice(self.payload());
        total_len
    }

    pub fn encoded_size(&self) -> usize {
        3 + self.length as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeState {
    Idle,
    LenHigh,
    LenLow,
    Payload,
}

#[derive(Debug)]
pub struct FrameDecoder {
    state: DecodeState,
    command_byte: u8,
    length: u16,
    payload_idx: usize,
    payload: [u8; MAX_PAYLOAD_SIZE],
}

impl FrameDecoder {
    pub const fn new() -> Self {
        Self {
            state: DecodeState::Idle,
            command_byte: 0,
            length: 0,
            payload_idx: 0,
            payload: [0; MAX_PAYLOAD_SIZE],
        }
    }

    pub fn reset(&mut self) {
        self.state = DecodeState::Idle;
        self.command_byte = 0;
        self.length = 0;
        self.payload_idx = 0;
    }

    pub fn decode(&mut self, data: &[u8]) -> Option<Frame> {
        for &byte in data {
            match self.state {
                DecodeState::Idle => {
                    self.command_byte = byte;
                    self.state = DecodeState::LenHigh;
                }
                DecodeState::LenHigh => {
                    self.length = (byte as u16) << 8;
                    self.state = DecodeState::LenLow;
                }
                DecodeState::LenLow => {
                    self.length |= byte as u16;

                    if self.length as usize > MAX_PAYLOAD_SIZE {
                        self.reset();
                        return None;
                    }

                    if self.length == 0 {
                        let cmd = Command::from_byte(self.command_byte)?;
                        let frame = Frame::new(cmd);
                        self.reset();
                        return Some(frame);
                    }

                    self.payload_idx = 0;
                    self.state = DecodeState::Payload;
                }
                DecodeState::Payload => {
                    self.payload[self.payload_idx] = byte;
                    self.payload_idx += 1;

                    if self.payload_idx >= self.length as usize {
                        let cmd = Command::from_byte(self.command_byte)?;
                        let frame = Frame::with_payload(cmd, &self.payload[..self.payload_idx])?;
                        self.reset();
                        return Some(frame);
                    }
                }
            }
        }
        None
    }
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "scanner-async"))]
pub struct CdcPort<'a> {
    serial: usbd_serial::SerialPort<'a, UsbBusType>,
    decoder: FrameDecoder,
    tx_buf: [u8; RX_BUF_SIZE],
}

#[cfg(not(feature = "scanner-async"))]
impl<'a> CdcPort<'a> {
    pub fn new(serial: usbd_serial::SerialPort<'a, UsbBusType>) -> Self {
        Self {
            serial,
            decoder: FrameDecoder::new(),
            tx_buf: [0; RX_BUF_SIZE],
        }
    }

    pub fn receive_frame(&mut self) -> Option<Frame> {
        let mut rx_buf = [0u8; 64];

        match self.serial.read(&mut rx_buf) {
            Ok(count) if count > 0 => self.decoder.decode(&rx_buf[..count]),
            _ => None,
        }
    }

    pub fn send_response(&mut self, response: &Response) -> bool {
        let len = response.encode(&mut self.tx_buf);
        if len == 0 {
            return false;
        }

        let mut offset = 0;
        while offset < len {
            match self.serial.write(&self.tx_buf[offset..len]) {
                Ok(written) if written > 0 => {
                    offset += written;
                }
                _ => {
                    let _ = self.serial.flush();
                }
            }
        }

        let _ = self.serial.flush();
        true
    }

    pub fn send_error(&mut self, status: Status) -> bool {
        let response = Response::new(status);
        self.send_response(&response)
    }

    pub fn serial_mut(&mut self) -> &mut usbd_serial::SerialPort<'a, UsbBusType> {
        &mut self.serial
    }
}
