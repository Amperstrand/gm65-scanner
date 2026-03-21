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
use crate::protocol;

const GM65_SUCCESS_PREFIX: [u8; 4] = [0x02, 0x00, 0x00, 0x01];
const GM65_SUCCESS_LEN: usize = 7;

const SERIAL_ADDR: [u8; 2] = [0x00, 0x0D];
const SETTINGS_ADDR: [u8; 2] = [0x00, 0x00];
const BAUD_RATE_ADDR: [u8; 2] = [0x00, 0x2A];
const BAUD_RATE_115200: [u8; 2] = [0x1A, 0x00];
const SCAN_ADDR: [u8; 2] = [0x00, 0x02];
const TIMEOUT_ADDR: [u8; 2] = [0x00, 0x06];
const SCAN_INTERVAL_ADDR: [u8; 2] = [0x00, 0x05];
const SAME_BARCODE_DELAY_ADDR: [u8; 2] = [0x00, 0x13];
const VERSION_ADDR: [u8; 2] = [0x00, 0xE2];
const VERSION_NEEDS_RAW: u8 = 0x69;
const RAW_MODE_ADDR: [u8; 2] = [0x00, 0xBC];
const RAW_MODE_VALUE: u8 = 0x08;
const BAR_TYPE_ADDR: [u8; 2] = [0x00, 0x2C];
const QR_ADDR: [u8; 2] = [0x00, 0x3F];

const SCAN_INTERVAL_MS: u8 = 0x01;
const SAME_BARCODE_DELAY: u8 = 0x85;
const CMD_MODE: u8 = 0xD1;

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
    /// Read one byte from UART if available. Returns None if no data.
    /// Does not block.
    pub fn poll_uart(&mut self) -> Option<u8> {
        match self.uart.read() {
            Ok(b) => Some(b),
            Err(nb::Error::WouldBlock) => None,
            Err(_) => None,
        }
    }

    /// Non-blocking: try to read one byte into the scan buffer.
    /// Returns Some(data) if a complete scan was received, None otherwise.
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

    fn send_command(&mut self, cmd: &[u8]) -> Option<Vec<u8>> {
        if self.uart_write_all(cmd).is_err() {
            return None;
        }

        let mut resp = Vec::with_capacity(GM65_SUCCESS_LEN);
        let mut total_attempts = 0u32;
        let max_attempts = 200_000u32;

        while resp.len() < GM65_SUCCESS_LEN && total_attempts < max_attempts {
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

        if resp.len() != GM65_SUCCESS_LEN {
            return None;
        }

        Some(resp)
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

    fn get_setting(&mut self, addr: [u8; 2]) -> Option<u8> {
        let cmd = protocol::build_get_setting(addr);
        let resp = self.send_command(&cmd)?;

        if resp.len() != GM65_SUCCESS_LEN {
            return None;
        }
        if resp[0..4] != GM65_SUCCESS_PREFIX {
            return None;
        }

        Some(resp[4])
    }

    fn set_setting(&mut self, addr: [u8; 2], value: u8) -> bool {
        let cmd = protocol::build_set_setting(addr, value);
        match self.send_command(&cmd) {
            Some(resp) => resp.len() == GM65_SUCCESS_LEN && resp[0..4] == GM65_SUCCESS_PREFIX,
            None => false,
        }
    }

    fn set_setting_2byte(&mut self, addr: [u8; 2], value: [u8; 2]) -> bool {
        let cmd = protocol::build_set_setting_2byte(addr, value);
        match self.send_command(&cmd) {
            Some(resp) => resp.len() == GM65_SUCCESS_LEN && resp[0..4] == GM65_SUCCESS_PREFIX,
            None => false,
        }
    }

    fn save_settings(&mut self) -> bool {
        let cmd = protocol::build_save_settings();
        match self.send_command(&cmd) {
            Some(resp) => resp.len() == GM65_SUCCESS_LEN && resp[0..4] == GM65_SUCCESS_PREFIX,
            None => false,
        }
    }

    fn probe_gm65(&mut self) -> bool {
        self.drain_uart();
        self.get_setting(SERIAL_ADDR).is_some()
    }

    fn do_init(&mut self) -> Result<ScannerModel, ScannerError> {
        self.state = ScannerState::Detecting;

        if !self.probe_gm65() {
            self.state = ScannerState::Error(ScannerError::NotDetected);
            return Err(ScannerError::NotDetected);
        }

        self.detected_model = ScannerModel::Gm65;
        self.state = ScannerState::Configuring;

        let serial_val = match self.get_setting(SERIAL_ADDR) {
            Some(v) => v,
            None => {
                self.state = ScannerState::Error(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        };

        if serial_val & 0x03 != 0 {
            if !self.set_setting(SERIAL_ADDR, serial_val & 0xFC) {
                self.state = ScannerState::Error(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        }

        let scanner_settings: [([u8; 2], u8); 6] = [
            (SETTINGS_ADDR, CMD_MODE),
            (TIMEOUT_ADDR, 0x00),
            (SCAN_INTERVAL_ADDR, SCAN_INTERVAL_MS),
            (SAME_BARCODE_DELAY_ADDR, SAME_BARCODE_DELAY),
            (BAR_TYPE_ADDR, 0x01),
            (QR_ADDR, 0x01),
        ];

        for (addr, set_val) in scanner_settings.iter() {
            match self.get_setting(*addr) {
                Some(val) => {
                    if val != *set_val {
                        if !self.set_setting(*addr, *set_val) {
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

        if let Some(version) = self.get_setting(VERSION_ADDR) {
            if version == VERSION_NEEDS_RAW {
                if let Some(val) = self.get_setting(RAW_MODE_ADDR) {
                    if val != RAW_MODE_VALUE {
                        self.set_setting(RAW_MODE_ADDR, RAW_MODE_VALUE);
                    }
                }
            }
        }

        let _ = self.save_settings();

        self.set_setting_2byte(BAUD_RATE_ADDR, BAUD_RATE_115200);

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
        let cmd = protocol::build_set_setting(SCAN_ADDR, 0x01);
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
        self.get_setting(SERIAL_ADDR).is_some()
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
        core::future::ready(self.get_setting(SERIAL_ADDR).is_some())
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
