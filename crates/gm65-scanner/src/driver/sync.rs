//! Sync (blocking) GM65 scanner implementation.
//!
//! This module provides `Gm65Scanner<UART>` with blocking I/O operations
//! using `embedded-hal-02` traits.

extern crate alloc;

use alloc::vec::Vec;

use crate::buffer::ScanBuffer;
use crate::driver::{
    ScannerConfig, ScannerDriverSync, ScannerError, ScannerModel, ScannerState, ScannerStatus,
};
use crate::protocol::{self, Gm65Response, Register, RESPONSE_LEN};
use crate::state_machine::{config, ScannerSettings};

/// GM65/M3Y QR scanner driver (blocking/sync version).
///
/// This type implements `ScannerDriverSync` using blocking UART operations.
/// It uses `embedded-hal-02` traits for compatibility with existing HALs.
///
/// # Example
///
/// ```rust,ignore
/// use gm65_scanner::{Gm65Scanner, ScannerDriverSync, ScannerConfig};
///
/// let mut scanner = Gm65Scanner::new(uart, ScannerConfig::default());
/// scanner.init()?;
/// scanner.trigger_scan()?;
/// if let Some(data) = scanner.read_scan() {
///     // process QR code data
/// }
/// ```
pub struct Gm65Scanner<UART> {
    uart: UART,
    config: ScannerConfig,
    state: ScannerState,
    buffer: ScanBuffer,
    initialized: bool,
    detected_model: ScannerModel,
    last_scan_len: Option<usize>,
}

impl<UART, WErr, RErr> Gm65Scanner<UART>
where
    UART: embedded_hal_02::serial::Write<u8, Error = WErr>
        + embedded_hal_02::serial::Read<u8, Error = RErr>,
{
    /// Create a new scanner with the given UART and configuration.
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

    /// Create a new scanner with default configuration.
    pub fn with_default_config(uart: UART) -> Self {
        Self::new(uart, ScannerConfig::default())
    }

    /// Release ownership of the UART peripheral.
    pub fn release(self) -> UART {
        self.uart
    }

    /// Decompose into parts for recovery or reconfiguration.
    pub fn into_parts(self) -> (UART, ScannerState, bool, ScannerModel) {
        (self.uart, self.state, self.initialized, self.detected_model)
    }

    /// Get scanner settings as bitflags.
    pub fn get_scanner_settings(&mut self) -> Option<ScannerSettings> {
        self.get_setting(Register::Settings)
            .and_then(|v| ScannerSettings::from_bits(v))
    }

    /// Set scanner settings from bitflags.
    pub fn set_scanner_settings(&mut self, settings: ScannerSettings) -> bool {
        self.set_setting(Register::Settings, settings.bits())
    }

    /// Poll UART for a single byte (non-blocking).
    pub fn poll_uart(&mut self) -> Option<u8> {
        match self.uart.read() {
            Ok(b) => Some(b),
            Err(nb::Error::WouldBlock) => None,
            Err(_) => None,
        }
    }

    /// Non-blocking incremental scan read.
    /// Call repeatedly until `data_ready()` returns true.
    pub fn try_read_scan(&mut self) -> Option<Vec<u8>> {
        if !self.initialized {
            return None;
        }
        if self.state == ScannerState::ScanComplete {
            self.state = ScannerState::Ready;
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

    // ========================================================================
    // Internal UART Operations
    // ========================================================================

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
        self.drain_uart();
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
                    if attempts > 50_000 {
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
        self.send_command(&cmd)
            .map_or(false, |r| r != Gm65Response::Invalid)
    }

    fn save_settings(&mut self) -> bool {
        let cmd = protocol::build_save_settings();
        self.send_command(&cmd)
            .map_or(false, |r| r != Gm65Response::Invalid)
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

        // Read SerialOutput with retry
        #[allow(unused_variables)]
        let serial_val = {
            let mut result = None;
            for attempt in 0..3u32 {
                self.drain_uart();
                match self.get_setting(Register::SerialOutput) {
                    Some(v) => {
                        result = Some(v);
                        break;
                    }
                    None => {
                        #[cfg(feature = "defmt")]
                        defmt::warn!("SerialOutput read failed, retry {}...", attempt + 1);
                        for _ in 0..1_000_000 {
                            core::hint::spin_loop();
                        }
                    }
                }
            }
            match result {
                Some(v) => {
                    #[cfg(feature = "defmt")]
                    defmt::info!("SerialOutput: 0x{:02x}", v);
                    v
                }
                None => {
                    #[cfg(feature = "defmt")]
                    defmt::warn!("init: failed to read SerialOutput after retries");
                    self.state = ScannerState::Error(ScannerError::ConfigFailed);
                    return Err(ScannerError::ConfigFailed);
                }
            }
        };

        // Fix SerialOutput if needed (clear bits 0-1)
        if serial_val & 0x03 != 0 {
            let fixed = serial_val & 0xFC;
            if !self.set_setting(Register::SerialOutput, fixed) {
                #[cfg(feature = "defmt")]
                defmt::warn!("init: failed to fix SerialOutput");
                self.state = ScannerState::Error(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        }

        // Set command mode
        match self.get_setting(Register::Settings) {
            Some(val) => {
                #[cfg(feature = "defmt")]
                defmt::info!("Settings: 0x{:02x}", val);
                if val != config::CMD_MODE {
                    if !self.set_setting(Register::Settings, config::CMD_MODE) {
                        #[cfg(feature = "defmt")]
                        defmt::warn!("init: failed to set Settings to CMD_MODE");
                        self.state = ScannerState::Error(ScannerError::ConfigFailed);
                        return Err(ScannerError::ConfigFailed);
                    }
                }
            }
            None => {
                #[cfg(feature = "defmt")]
                defmt::warn!("init: failed to read Settings");
                self.state = ScannerState::Error(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        }

        // Apply configuration settings
        let config_settings = crate::state_machine::init_config_sequence();

        for (reg, set_val) in config_settings.iter() {
            match self.get_setting(*reg) {
                Some(val) => {
                    if val != *set_val {
                        #[cfg(feature = "defmt")]
                        defmt::info!(
                            "Setting {:02x}: 0x{:02x} -> 0x{:02x}",
                            reg.address_bytes(),
                            val,
                            set_val
                        );
                        if !self.set_setting(*reg, *set_val) {
                            #[cfg(feature = "defmt")]
                            defmt::warn!(
                                "init: failed to set register {:02x}",
                                reg.address_bytes()
                            );
                            self.state = ScannerState::Error(ScannerError::ConfigFailed);
                            return Err(ScannerError::ConfigFailed);
                        }
                    }
                }
                None => {
                    #[cfg(feature = "defmt")]
                    defmt::warn!("init: failed to read register {:02x}", reg.address_bytes());
                    self.state = ScannerState::Error(ScannerError::ConfigFailed);
                    return Err(ScannerError::ConfigFailed);
                }
            }
        }

        // Check firmware version for raw mode fix
        if let Some(version) = self.get_setting(Register::Version) {
            #[cfg(feature = "defmt")]
            defmt::info!("Firmware version: 0x{:02x}", version);
            if crate::state_machine::version_needs_raw_fix(version) {
                if let Some(val) = self.get_setting(Register::RawMode) {
                    if val != config::RAW_MODE_VALUE {
                        self.set_setting(Register::RawMode, config::RAW_MODE_VALUE);
                    }
                }
            }
        }

        let _ = self.save_settings();
        #[cfg(feature = "defmt")]
        defmt::info!("init: complete");

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
        let cmd = protocol::build_set_setting(Register::ScanEnable.address_bytes(), 0x01);
        let _ = self.send_command(&cmd);
        Ok(())
    }

    fn do_stop_scan(&mut self) -> bool {
        if !self.initialized {
            return false;
        }
        let cmd = protocol::build_set_setting(Register::ScanEnable.address_bytes(), 0x00);
        self.send_command(&cmd).is_some()
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

    fn stop_scan(&mut self) -> bool {
        self.do_stop_scan()
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
