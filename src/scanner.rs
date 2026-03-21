//! GM65 Scanner Implementation
//!
//! Concrete `Gm65Scanner<UART>` type implementing both `ScannerDriverSync` and
//! `ScannerDriver` (async) traits. Requires the `embedded-hal` feature.

extern crate alloc;

use alloc::vec::Vec;

use crate::buffer::ScanBuffer;
use crate::driver::{
    ScannerConfig, ScannerDriverSync, ScannerError, ScannerModel, ScannerState, ScannerStatus,
};
use crate::protocol::{self, Gm65Response, Register, RESPONSE_LEN};

const BAUD_RATE_115200: [u8; 2] = [0x1A, 0x00];
const SCAN_INTERVAL_MS: u8 = 0x01;
const SAME_BARCODE_DELAY: u8 = 0x85;
const CMD_MODE: u8 = 0xD1;
const VERSION_NEEDS_RAW: u8 = 0x69;
const RAW_MODE_VALUE: u8 = 0x08;

pub struct Gm65Scanner<UART> {
    uart: UART,
    config: ScannerConfig,
    state: ScannerState,
    buffer: ScanBuffer,
    initialized: bool,
    detected_model: ScannerModel,
    last_scan_len: Option<usize>,
}

impl<UART> Gm65Scanner<UART> {
    pub fn new(uart: UART, config: ScannerConfig) -> Self {
        Self {
            uart,
            config,
            state: ScannerState::Uninitialized,
            buffer: ScanBuffer::new(),
            initialized: false,
            detected_model: ScannerModel::Unknown,
            last_scan_len: None,
        }
    }

    pub fn with_default_config(uart: UART) -> Self {
        Self::new(uart, ScannerConfig::default())
    }

    pub fn release(self) -> UART {
        self.uart
    }

    pub fn into_parts(self) -> (UART, ScannerState, bool, ScannerModel) {
        (self.uart, self.state, self.initialized, self.detected_model)
    }
}

impl<UART, WErr, RErr> Gm65Scanner<UART>
where
    UART: embedded_hal_02::serial::Write<u8, Error = WErr>
        + embedded_hal_02::serial::Read<u8, Error = RErr>,
{
    pub fn poll_uart(&mut self) -> Option<u8> {
        match self.uart.read() {
            Ok(b) => Some(b),
            Err(nb::Error::WouldBlock) => None,
            Err(_) => None,
        }
    }

    pub fn try_read_scan(&mut self) -> Option<Vec<u8>> {
        if !self.initialized {
            return None;
        }
        match self.poll_uart() {
            Some(b) => {
                if !self.buffer.push(b) {
                    self.state = ScannerState::Error(ScannerError::BufferOverflow);
                    return None;
                }
                if self.buffer.has_eol() {
                    let data = self.buffer.data_without_eol();
                    if data.is_empty() {
                        self.buffer.clear();
                        return None;
                    }
                    self.last_scan_len = Some(data.len());
                    self.state = ScannerState::ScanComplete;
                    let result = data.to_vec();
                    self.buffer.clear();
                    return Some(result);
                }
                None
            }
            None => None,
        }
    }

    fn uart_write_all(&mut self, data: &[u8]) -> Result<(), ()> {
        for &byte in data {
            let mut attempts = 0u32;
            loop {
                match self.uart.write(byte) {
                    Ok(()) => break,
                    Err(nb::Error::WouldBlock) => {
                        attempts += 1;
                        if attempts > 100_000 {
                            return Err(());
                        }
                    }
                    Err(nb::Error::Other(_)) => return Err(()),
                }
            }
        }
        self.uart.flush().ok();
        Ok(())
    }

    fn send_command(&mut self, cmd: &[u8]) -> Option<Gm65Response> {
        if self.uart_write_all(cmd).is_err() {
            return None;
        }

        let mut resp = Vec::with_capacity(RESPONSE_LEN);
        let mut total_attempts = 0u32;
        let max_attempts = 200_000u32;

        while resp.len() < RESPONSE_LEN && total_attempts < max_attempts {
            match self.uart.read() {
                Ok(byte) => {
                    resp.push(byte);
                    total_attempts = 0;
                }
                Err(nb::Error::WouldBlock) => {
                    total_attempts += 1;
                }
                Err(_) => {
                    return None;
                }
            }
        }

        if resp.len() != RESPONSE_LEN {
            return None;
        }

        let parsed = Gm65Response::parse(&resp);
        if parsed == Gm65Response::Invalid {
            return None;
        }

        Some(parsed)
    }

    fn drain_uart(&mut self) {
        let mut attempts = 0u32;
        loop {
            match self.uart.read() {
                Ok(_) => attempts = 0,
                Err(nb::Error::WouldBlock) => {
                    attempts += 1;
                    if attempts > 1000 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }

    fn get_setting(&mut self, reg: Register) -> Option<u8> {
        let cmd = protocol::build_get_setting(reg.address_bytes());
        match self.send_command(&cmd) {
            Some(Gm65Response::SuccessWithValue(v)) => Some(v),
            _ => None,
        }
    }

    fn set_setting(&mut self, reg: Register, value: u8) -> bool {
        let cmd = protocol::build_set_setting(reg.address_bytes(), value);
        matches!(self.send_command(&cmd), Some(Gm65Response::Success))
    }

    fn set_setting_2byte(&mut self, reg: Register, value: [u8; 2]) -> bool {
        let cmd = protocol::build_set_setting_2byte(reg.address_bytes(), value);
        matches!(self.send_command(&cmd), Some(Gm65Response::Success))
    }

    fn save_settings(&mut self) -> bool {
        let cmd = protocol::build_save_settings();
        matches!(self.send_command(&cmd), Some(Gm65Response::Success))
    }

    fn probe_gm65(&mut self) -> bool {
        self.drain_uart();
        self.get_setting(Register::SerialOutput).is_some()
    }

    fn do_init(&mut self) -> Result<ScannerModel, ScannerError> {
        self.state = ScannerState::Detecting;

        if !self.probe_gm65() {
            self.state = ScannerState::Error(ScannerError::NotDetected);
            return Err(ScannerError::NotDetected);
        }

        self.detected_model = ScannerModel::Gm65;
        self.state = ScannerState::Configuring;

        let serial_val = match self.get_setting(Register::SerialOutput) {
            Some(v) => v,
            None => {
                self.state = ScannerState::Error(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        };

        if serial_val & 0x03 != 0 {
            if !self.set_setting(Register::SerialOutput, serial_val & 0xFC) {
                self.state = ScannerState::Error(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        }

        let scanner_settings: [(Register, u8); 6] = [
            (Register::Settings, CMD_MODE),
            (Register::Timeout, 0x00),
            (Register::ScanInterval, SCAN_INTERVAL_MS),
            (Register::SameBarcodeDelay, SAME_BARCODE_DELAY),
            (Register::BarType, 0x01),
            (Register::QrEnable, 0x01),
        ];

        for (reg, set_val) in scanner_settings.iter() {
            match self.get_setting(*reg) {
                Some(val) => {
                    if val != *set_val {
                        if !self.set_setting(*reg, *set_val) {
                            self.state = ScannerState::Error(ScannerError::ConfigFailed);
                            return Err(ScannerError::ConfigFailed);
                        }
                    }
                }
                None => {
                    self.state = ScannerState::Error(ScannerError::ConfigFailed);
                    return Err(ScannerError::ConfigFailed);
                }
            }
        }

        if let Some(version) = self.get_setting(Register::Version) {
            if version == VERSION_NEEDS_RAW {
                if let Some(val) = self.get_setting(Register::RawMode) {
                    if val != RAW_MODE_VALUE {
                        self.set_setting(Register::RawMode, RAW_MODE_VALUE);
                    }
                }
            }
        }

        let _ = self.save_settings();

        self.set_setting_2byte(Register::BaudRate, BAUD_RATE_115200);

        self.initialized = true;
        self.state = ScannerState::Ready;
        self.config.model = self.detected_model;
        Ok(self.detected_model)
    }

    fn do_trigger_scan(&mut self) -> Result<(), ScannerError> {
        if !self.initialized {
            return Err(ScannerError::NotInitialized);
        }
        self.state = ScannerState::Scanning;
        self.buffer.clear();
        self.drain_uart();
        let cmd = protocol::build_set_setting(Register::ScanEnable.address_bytes(), 0x01);
        self.uart_write_all(&cmd).ok();
        Ok(())
    }

    fn do_read_scan(&mut self) -> Option<Vec<u8>> {
        if !self.initialized {
            return None;
        }

        let mut attempts = 0u32;
        let max_attempts = 500_000u32;

        while attempts < max_attempts {
            match self.uart.read() {
                Ok(b) => {
                    if !self.buffer.push(b) {
                        self.state = ScannerState::Error(ScannerError::BufferOverflow);
                        return None;
                    }
                    if self.buffer.has_eol() {
                        let data = self.buffer.data_without_eol();
                        if data.is_empty() {
                            self.buffer.clear();
                            return None;
                        }
                        self.last_scan_len = Some(data.len());
                        self.state = ScannerState::ScanComplete;
                        let result = data.to_vec();
                        self.buffer.clear();
                        return Some(result);
                    }
                    attempts = 0;
                }
                Err(nb::Error::WouldBlock) => {
                    attempts += 1;
                }
                Err(_) => {
                    self.state = ScannerState::Error(ScannerError::UartError);
                    return None;
                }
            }
        }

        self.state = ScannerState::Error(ScannerError::Timeout);
        None
    }
}

impl<UART, WErr, RErr> ScannerDriverSync for Gm65Scanner<UART>
where
    UART: embedded_hal_02::serial::Write<u8, Error = WErr>
        + embedded_hal_02::serial::Read<u8, Error = RErr>,
{
    fn init(&mut self) -> Result<ScannerModel, ScannerError> {
        self.do_init()
    }

    fn ping(&mut self) -> bool {
        self.get_setting(Register::SerialOutput).is_some()
    }

    fn trigger_scan(&mut self) -> Result<(), ScannerError> {
        self.do_trigger_scan()
    }

    fn read_scan(&mut self) -> Option<Vec<u8>> {
        self.do_read_scan()
    }

    fn state(&self) -> ScannerState {
        self.state
    }

    fn status(&self) -> ScannerStatus {
        ScannerStatus {
            model: self.detected_model,
            connected: self.initialized,
            initialized: self.initialized,
            config: self.config.clone(),
            last_scan_len: self.last_scan_len,
        }
    }

    fn data_ready(&self) -> bool {
        self.state == ScannerState::ScanComplete
    }
}

#[cfg(feature = "embedded-hal-async")]
use crate::driver::ScannerDriver;

#[cfg(feature = "embedded-hal-async")]
impl<UART, WErr, RErr> ScannerDriver for Gm65Scanner<UART>
where
    UART: embedded_hal_02::serial::Write<u8, Error = WErr>
        + embedded_hal_02::serial::Read<u8, Error = RErr>
        + Send,
{
    fn init(
        &mut self,
    ) -> impl core::future::Future<Output = Result<ScannerModel, ScannerError>> + Send {
        core::future::ready(self.do_init())
    }

    fn ping(&mut self) -> impl core::future::Future<Output = bool> + Send {
        core::future::ready(self.get_setting(Register::SerialOutput).is_some())
    }

    fn trigger_scan(
        &mut self,
    ) -> impl core::future::Future<Output = Result<(), ScannerError>> + Send {
        core::future::ready(self.do_trigger_scan())
    }

    fn read_scan(&mut self) -> impl core::future::Future<Output = Option<Vec<u8>>> + Send {
        core::future::ready(self.do_read_scan())
    }

    fn state(&self) -> ScannerState {
        self.state
    }

    fn status(&self) -> ScannerStatus {
        ScannerStatus {
            model: self.detected_model,
            connected: self.initialized,
            initialized: self.initialized,
            config: self.config.clone(),
            last_scan_len: self.last_scan_len,
        }
    }

    fn data_ready(&self) -> bool {
        self.state == ScannerState::ScanComplete
    }
}
