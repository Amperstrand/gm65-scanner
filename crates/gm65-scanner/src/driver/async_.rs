//! Async GM65 scanner implementation.
//!
//! This module provides `Gm65ScannerAsync<UART>` with true async I/O operations
//! using `embedded-io-async` traits.

extern crate alloc;

use alloc::vec::Vec;

use crate::buffer::ScanBuffer;
use crate::driver::{
    ScannerConfig, ScannerDriver, ScannerError, ScannerModel, ScannerState, ScannerStatus,
};
use crate::protocol::{self, Gm65Response, Register, RESPONSE_LEN};
use crate::state_machine::{config, ScannerSettings};

/// GM65/M3Y QR scanner driver (async version).
///
/// This type implements `ScannerDriver` using true async UART operations.
/// It uses `embedded-io-async` traits for compatibility with async executors.
///
/// # Example
///
/// ```rust,ignore
/// use gm65_scanner::{Gm65ScannerAsync, ScannerDriver, ScannerConfig};
///
/// let mut scanner = Gm65ScannerAsync::new(uart, ScannerConfig::default());
/// scanner.init().await?;
/// scanner.trigger_scan().await?;
/// if let Some(data) = scanner.read_scan().await {
///     // process QR code data
/// }
/// ```
pub struct Gm65ScannerAsync<UART> {
    uart: UART,
    config: ScannerConfig,
    state: ScannerState,
    buffer: ScanBuffer,
    initialized: bool,
    detected_model: ScannerModel,
    last_scan_len: Option<usize>,
}

impl<UART> Gm65ScannerAsync<UART> {
    /// Create a new async scanner with the given UART and configuration.
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

    /// Create a new async scanner with default configuration.
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
    pub async fn get_scanner_settings(&mut self) -> Option<ScannerSettings>
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        self.get_setting(Register::Settings)
            .await
            .and_then(|v| ScannerSettings::from_bits(v))
    }

    /// Set scanner settings from bitflags.
    pub async fn set_scanner_settings(&mut self, settings: ScannerSettings) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        self.set_setting(Register::Settings, settings.bits()).await
    }

    /// Non-blocking incremental scan read.
    /// Call repeatedly until `data_ready()` returns true.
    /// Returns `Some(data)` when a complete scan is available.
    pub async fn try_read_scan(&mut self) -> Option<Vec<u8>>
    where
        UART: embedded_io_async::Read,
    {
        if !self.initialized {
            return None;
        }
        if self.state == ScannerState::ScanComplete {
            self.state = ScannerState::Ready;
        }

        let mut byte_buf = [0u8; 1];
        match self.uart.read(&mut byte_buf).await {
            Ok(0) => None, // No data available
            Ok(_) => {
                let b = byte_buf[0];
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
            Err(_) => None,
        }
    }

    // ========================================================================
    // Internal UART Operations (Async)
    // ========================================================================

    async fn uart_write_all(&mut self, data: &[u8]) -> Result<(), ()>
    where
        UART: embedded_io_async::Write,
    {
        self.uart.write_all(data).await.map_err(|_| ())
    }

    async fn send_command(&mut self, cmd: &[u8]) -> Option<Gm65Response>
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        self.drain_uart().await;
        if self.uart_write_all(cmd).await.is_err() {
            return None;
        }

        let mut resp = Vec::with_capacity(RESPONSE_LEN);
        let mut buf = [0u8; 1];

        // Read response bytes one at a time
        // In true async, we should have a timeout here, but for simplicity
        // we rely on the underlying HAL's behavior
        while resp.len() < RESPONSE_LEN {
            match self.uart.read(&mut buf).await {
                Ok(0) => {
                    // No data - return None if we haven't started receiving
                    if resp.is_empty() {
                        return None;
                    }
                    // Keep trying if we have partial data (could add timeout here)
                }
                Ok(_) => {
                    resp.push(buf[0]);
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

    async fn drain_uart(&mut self)
    where
        UART: embedded_io_async::Read,
    {
        let mut buf = [0u8; 16];
        // Drain in chunks for efficiency
        loop {
            match self.uart.read(&mut buf).await {
                Ok(0) => break, // No more data
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }

    async fn get_setting(&mut self, reg: Register) -> Option<u8>
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        let cmd = protocol::build_get_setting(reg.address_bytes());
        match self.send_command(&cmd).await {
            Some(Gm65Response::SuccessWithValue(v)) => Some(v),
            _ => None,
        }
    }

    async fn set_setting(&mut self, reg: Register, value: u8) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        let cmd = protocol::build_set_setting(reg.address_bytes(), value);
        self.send_command(&cmd)
            .await
            .map_or(false, |r| r != Gm65Response::Invalid)
    }

    async fn save_settings(&mut self) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        let cmd = protocol::build_save_settings();
        self.send_command(&cmd)
            .await
            .map_or(false, |r| r != Gm65Response::Invalid)
    }

    async fn probe_gm65(&mut self) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        self.drain_uart().await;
        self.get_setting(Register::SerialOutput).await.is_some()
    }

    async fn do_init(&mut self) -> Result<ScannerModel, ScannerError>
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        self.state = ScannerState::Detecting;

        if !self.probe_gm65().await {
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
                self.drain_uart().await;
                match self.get_setting(Register::SerialOutput).await {
                    Some(v) => {
                        result = Some(v);
                        break;
                    }
                    None => {
                        #[cfg(feature = "defmt")]
                        defmt::warn!("SerialOutput read failed, retry {}...", attempt + 1);
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
            if !self.set_setting(Register::SerialOutput, fixed).await {
                #[cfg(feature = "defmt")]
                defmt::warn!("init: failed to fix SerialOutput");
                self.state = ScannerState::Error(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        }

        // Set command mode
        match self.get_setting(Register::Settings).await {
            Some(val) => {
                #[cfg(feature = "defmt")]
                defmt::info!("Settings: 0x{:02x}", val);
                if val != config::CMD_MODE {
                    if !self.set_setting(Register::Settings, config::CMD_MODE).await {
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
            match self.get_setting(*reg).await {
                Some(val) => {
                    if val != *set_val {
                        #[cfg(feature = "defmt")]
                        defmt::info!(
                            "Setting {:02x}: 0x{:02x} -> 0x{:02x}",
                            reg.address_bytes(),
                            val,
                            set_val
                        );
                        if !self.set_setting(*reg, *set_val).await {
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
        if let Some(version) = self.get_setting(Register::Version).await {
            #[cfg(feature = "defmt")]
            defmt::info!("Firmware version: 0x{:02x}", version);
            if crate::state_machine::version_needs_raw_fix(version) {
                if let Some(val) = self.get_setting(Register::RawMode).await {
                    if val != config::RAW_MODE_VALUE {
                        self.set_setting(Register::RawMode, config::RAW_MODE_VALUE)
                            .await;
                    }
                }
            }
        }

        let _ = self.save_settings().await;
        #[cfg(feature = "defmt")]
        defmt::info!("init: complete");

        self.initialized = true;
        self.state = ScannerState::Ready;
        self.config.model = self.detected_model;
        Ok(self.detected_model)
    }

    async fn do_trigger_scan(&mut self) -> Result<(), ScannerError>
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        if !self.initialized {
            return Err(ScannerError::NotInitialized);
        }
        self.state = ScannerState::Scanning;
        self.buffer.clear();
        let cmd = protocol::build_set_setting(Register::ScanEnable.address_bytes(), 0x01);
        let _ = self.send_command(&cmd).await;
        Ok(())
    }

    async fn do_stop_scan(&mut self) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        if !self.initialized {
            return false;
        }
        let cmd = protocol::build_set_setting(Register::ScanEnable.address_bytes(), 0x00);
        self.send_command(&cmd).await.is_some()
    }

    async fn do_read_scan(&mut self) -> Option<Vec<u8>>
    where
        UART: embedded_io_async::Read,
    {
        if !self.initialized {
            return None;
        }

        let mut buf = [0u8; 1];

        loop {
            match self.uart.read(&mut buf).await {
                Ok(0) => {
                    // No data available yet - return None, caller should retry
                    return None;
                }
                Ok(_) => {
                    let b = buf[0];
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
                    // Continue reading until we get EOL or error
                }
                Err(_) => {
                    self.state = ScannerState::Error(ScannerError::UartError);
                    return None;
                }
            }
        }
    }
}

impl<UART> ScannerDriver for Gm65ScannerAsync<UART>
where
    UART: embedded_io_async::Write + embedded_io_async::Read,
{
    async fn init(&mut self) -> Result<ScannerModel, ScannerError> {
        self.do_init().await
    }

    async fn ping(&mut self) -> bool {
        self.get_setting(Register::SerialOutput).await.is_some()
    }

    async fn trigger_scan(&mut self) -> Result<(), ScannerError> {
        self.do_trigger_scan().await
    }

    async fn stop_scan(&mut self) -> bool {
        self.do_stop_scan().await
    }

    async fn read_scan(&mut self) -> Option<Vec<u8>> {
        self.do_read_scan().await
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

#[cfg(feature = "hil-tests")]
pub mod hil_tests {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    pub struct HilTestResults {
        pub init_detects_scanner: bool,
        pub ping_after_init: bool,
        pub trigger_and_stop: bool,
        pub read_scan_timeout: bool,
        pub state_transitions: bool,
    }

    impl HilTestResults {
        pub fn all_passed(&self) -> bool {
            self.init_detects_scanner
                && self.ping_after_init
                && self.trigger_and_stop
                && self.read_scan_timeout
                && self.state_transitions
        }

        pub fn passed_count(&self) -> usize {
            [
                self.init_detects_scanner,
                self.ping_after_init,
                self.trigger_and_stop,
                self.read_scan_timeout,
                self.state_transitions,
            ]
            .iter()
            .filter(|&&x| x)
            .count()
        }
    }

    pub async fn run_hil_tests<UART>(
        scanner: &mut Gm65ScannerAsync<UART>,
    ) -> HilTestResults
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        defmt::info!("==== HIL TESTS STARTING ====");

        let mut results = HilTestResults {
            init_detects_scanner: false,
            ping_after_init: false,
            trigger_and_stop: false,
            read_scan_timeout: false,
            state_transitions: false,
        };

        defmt::info!("HIL: test_init_detects_scanner");
        results.init_detects_scanner = test_init(scanner).await;
        defmt::info!(
            "HIL: {} - init_detects_scanner",
            if results.init_detects_scanner { "PASS" } else { "FAIL" }
        );

        if !results.init_detects_scanner {
            defmt::warn!("HIL: Aborting remaining tests - no scanner detected");
            return results;
        }

        defmt::info!("HIL: test_ping_after_init");
        results.ping_after_init = test_ping(scanner).await;
        defmt::info!(
            "HIL: {} - ping_after_init",
            if results.ping_after_init { "PASS" } else { "FAIL" }
        );

        defmt::info!("HIL: test_trigger_and_stop");
        results.trigger_and_stop = test_trigger_stop(scanner).await;
        defmt::info!(
            "HIL: {} - trigger_and_stop",
            if results.trigger_and_stop { "PASS" } else { "FAIL" }
        );

        defmt::info!("HIL: test_read_scan_timeout");
        results.read_scan_timeout = test_read_scan_timeout(scanner).await;
        defmt::info!(
            "HIL: {} - read_scan_timeout",
            if results.read_scan_timeout { "PASS" } else { "FAIL" }
        );

        defmt::info!("HIL: test_state_transitions");
        results.state_transitions = test_state_transitions(scanner).await;
        defmt::info!(
            "HIL: {} - state_transitions",
            if results.state_transitions { "PASS" } else { "FAIL" }
        );

        defmt::info!(
            "==== HIL TESTS COMPLETE: {}/5 passed ====",
            results.passed_count()
        );

        results
    }

    async fn test_init<UART>(scanner: &mut Gm65ScannerAsync<UART>) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        match scanner.init().await {
            Ok(model) => {
                defmt::info!("HIL: detected model = {:?}", model);
                true
            }
            Err(e) => {
                defmt::error!("HIL: init failed with {:?}", e);
                false
            }
        }
    }

    async fn test_ping<UART>(scanner: &mut Gm65ScannerAsync<UART>) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        let result = scanner.ping().await;
        if !result {
            defmt::warn!("HIL: ping returned false");
        }
        result
    }

    async fn test_trigger_stop<UART>(scanner: &mut Gm65ScannerAsync<UART>) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        if scanner.trigger_scan().await.is_err() {
            defmt::error!("HIL: trigger_scan failed");
            return false;
        }

        if !matches!(scanner.state(), ScannerState::Scanning) {
            defmt::error!("HIL: state not Scanning after trigger");
            return false;
        }

        if !scanner.stop_scan().await {
            defmt::error!("HIL: stop_scan failed");
            return false;
        }

        true
    }

    async fn test_read_scan_timeout<UART>(scanner: &mut Gm65ScannerAsync<UART>) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        if scanner.trigger_scan().await.is_err() {
            defmt::error!("HIL: trigger_scan failed in timeout test");
            return false;
        }

        let result = scanner.read_scan().await;
        let pass = result.is_none()
            && matches!(
                scanner.state(),
                ScannerState::Error(ScannerError::Timeout)
            );

        let _ = scanner.stop_scan().await;

        if !pass {
            defmt::warn!("HIL: read_scan did not timeout as expected");
        }
        pass
    }

    async fn test_state_transitions<UART>(scanner: &mut Gm65ScannerAsync<UART>) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        let initial_state = scanner.state();
        defmt::info!("HIL: initial state = {:?}", initial_state);

        match scanner.init().await {
            Ok(_) => {
                let final_state = scanner.state();
                defmt::info!("HIL: final state = {:?}", final_state);
                matches!(final_state, ScannerState::Ready)
            }
            Err(e) => {
                defmt::error!("HIL: re-init failed: {:?}", e);
                false
            }
        }
    }

    pub async fn run_hil_test_with_qr<UART>(scanner: &mut Gm65ScannerAsync<UART>) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        defmt::info!("==== HIL TEST: SCAN WITH QR ====");
        defmt::info!("HIL: Present QR code within 5 seconds...");

        if scanner.trigger_scan().await.is_err() {
            defmt::error!("HIL: trigger failed");
            return false;
        }

        let result = scanner.read_scan().await;

        match result {
            Some(payload) => {
                defmt::info!("HIL: PASS - scanned {} bytes", payload.len());
                true
            }
            None => {
                defmt::error!("HIL: FAIL - no scan data");
                false
            }
        }
    }
}
