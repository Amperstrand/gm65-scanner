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

const SCAN_INTERVAL_MS: u8 = 0x01;
const SAME_BARCODE_DELAY: u8 = 0x85;
const CMD_MODE: u8 = 0xD1;
const VERSION_NEEDS_RAW: u8 = 0x69;
const RAW_MODE_VALUE: u8 = 0x08;

bitflags::bitflags! {
    #[derive(Clone, Copy)]
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

    pub fn get_scanner_settings(&mut self) -> Option<ScannerSettings> {
        self.get_setting(Register::Settings)
            .and_then(|v| ScannerSettings::from_bits(v))
    }

    pub fn set_scanner_settings(&mut self, settings: ScannerSettings) -> bool {
        self.set_setting(Register::Settings, settings.bits())
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

    #[allow(dead_code)]
    fn set_setting_2byte(&mut self, reg: Register, value: [u8; 2]) -> bool {
        let cmd = protocol::build_set_setting_2byte(reg.address_bytes(), value);
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

        if serial_val & 0x03 != 0 {
            let fixed = serial_val & 0xFC;
            if !self.set_setting(Register::SerialOutput, fixed) {
                #[cfg(feature = "defmt")]
                defmt::warn!("init: failed to fix SerialOutput");
                self.state = ScannerState::Error(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
            #[cfg(feature = "defmt")]
            defmt::info!(
                "SerialOutput fixed: 0x{:02x} -> 0x{:02x}",
                serial_val,
                fixed
            );
        }

        match self.get_setting(Register::Settings) {
            Some(val) => {
                #[cfg(feature = "defmt")]
                defmt::info!("Settings: 0x{:02x}", val);
                if val != CMD_MODE {
                    if !self.set_setting(Register::Settings, CMD_MODE) {
                        #[cfg(feature = "defmt")]
                        defmt::warn!("init: failed to set Settings to CMD_MODE");
                        self.state = ScannerState::Error(ScannerError::ConfigFailed);
                        return Err(ScannerError::ConfigFailed);
                    }
                    #[cfg(feature = "defmt")]
                    defmt::info!("Settings set to CMD_MODE (0xD1)");
                }
            }
            None => {
                #[cfg(feature = "defmt")]
                defmt::warn!("init: failed to read Settings");
                self.state = ScannerState::Error(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        }

        let config_settings: [(Register, u8); 5] = [
            (Register::Timeout, 0x00),
            (Register::ScanInterval, SCAN_INTERVAL_MS),
            (Register::SameBarcodeDelay, SAME_BARCODE_DELAY),
            (Register::BarType, 0x01),
            (Register::QrEnable, 0x01),
        ];

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
                    } else {
                        #[cfg(feature = "defmt")]
                        defmt::info!(
                            "Register {:02x}: already 0x{:02x}",
                            reg.address_bytes(),
                            val
                        );
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

        if let Some(version) = self.get_setting(Register::Version) {
            #[cfg(feature = "defmt")]
            defmt::info!("Firmware version: 0x{:02x}", version);
            if version == VERSION_NEEDS_RAW {
                if let Some(val) = self.get_setting(Register::RawMode) {
                    if val != RAW_MODE_VALUE {
                        self.set_setting(Register::RawMode, RAW_MODE_VALUE);
                    }
                }
            }
        } else {
            #[cfg(feature = "defmt")]
            defmt::warn!("init: failed to read Version");
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

    fn stop_scan(&mut self) -> impl core::future::Future<Output = bool> + Send {
        core::future::ready(self.do_stop_scan())
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
