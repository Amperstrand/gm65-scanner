//! Sync (blocking) GM65 scanner implementation.
//!
//! This module provides `Gm65Scanner<UART>` with blocking I/O operations
//! using `embedded-hal-02` traits. All state management is delegated to
//! `ScannerCore`.

extern crate alloc;

use alloc::vec::Vec;

use crate::driver::{
    ScannerConfig, ScannerDriverSync, ScannerError, ScannerModel, ScannerState, ScannerStatus,
};
use crate::protocol::{self, Gm65Response, Register, RESPONSE_LEN};
use crate::scanner_core::{
    config, init_config_sequence, version_needs_raw_fix, ScanByteResult, ScannerCore,
    ScannerSettings,
};

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
    core: ScannerCore,
    uart: UART,
}

impl<UART, WErr, RErr> Gm65Scanner<UART>
where
    UART: embedded_hal_02::serial::Write<u8, Error = WErr>
        + embedded_hal_02::serial::Read<u8, Error = RErr>,
{
    /// Create a new scanner with the given UART and configuration.
    pub fn new(uart: UART, config: ScannerConfig) -> Self {
        Self {
            core: ScannerCore::new(config),
            uart,
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
        (
            self.uart,
            self.core.state(),
            self.core.is_initialized(),
            self.core.detected_model(),
        )
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
        if !self.core.is_initialized() {
            return None;
        }
        // Reset state if previous scan complete
        if self.core.state() == ScannerState::ScanComplete {
            self.core.begin_scan().ok();
        }
        match self.poll_uart() {
            Some(byte) => match self.core.handle_scan_byte(byte) {
                ScanByteResult::Complete(data) => Some(data),
                ScanByteResult::BufferOverflow => None,
                ScanByteResult::NeedMore => None,
            },
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
        self.core.begin_init();

        if !self.probe_gm65() {
            self.core.fail_init(ScannerError::NotDetected);
            return Err(ScannerError::NotDetected);
        }

        self.core.mark_detected(ScannerModel::Gm65);

        // Read SerialOutput with retry
        let serial_val = {
            let mut result = None;
            for _attempt in 0..3u32 {
                self.drain_uart();
                match self.get_setting(Register::SerialOutput) {
                    Some(v) => {
                        result = Some(v);
                        break;
                    }
                    None => {
                        #[cfg(feature = "defmt")]
                        defmt::warn!("SerialOutput read failed, retry {}...", _attempt + 1);
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
                    self.core.fail_init(ScannerError::ConfigFailed);
                    return Err(ScannerError::ConfigFailed);
                }
            }
        };

        // Fix SerialOutput if needed (clear bits 0-1)
        if crate::scanner_core::serial_output_needs_fix(serial_val) {
            let fixed = crate::scanner_core::fix_serial_output(serial_val);
            if !self.set_setting(Register::SerialOutput, fixed) {
                #[cfg(feature = "defmt")]
                defmt::warn!("init: failed to fix SerialOutput");
                self.core.fail_init(ScannerError::ConfigFailed);
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
                        self.core.fail_init(ScannerError::ConfigFailed);
                        return Err(ScannerError::ConfigFailed);
                    }
                }
            }
            None => {
                #[cfg(feature = "defmt")]
                defmt::warn!("init: failed to read Settings");
                self.core.fail_init(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        }

        // Apply configuration settings
        let config_settings = init_config_sequence();

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
                            self.core.fail_init(ScannerError::ConfigFailed);
                            return Err(ScannerError::ConfigFailed);
                        }
                    }
                }
                None => {
                    #[cfg(feature = "defmt")]
                    defmt::warn!("init: failed to read register {:02x}", reg.address_bytes());
                    self.core.fail_init(ScannerError::ConfigFailed);
                    return Err(ScannerError::ConfigFailed);
                }
            }
        }

        // Check firmware version for raw mode fix
        if let Some(version) = self.get_setting(Register::Version) {
            #[cfg(feature = "defmt")]
            defmt::info!("Firmware version: 0x{:02x}", version);
            if version_needs_raw_fix(version) {
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

        self.core.complete_init(ScannerModel::Gm65);
        Ok(ScannerModel::Gm65)
    }

    fn do_trigger_scan(&mut self) -> Result<(), ScannerError> {
        self.core.begin_scan()?;
        let cmd = protocol::build_set_setting(Register::ScanEnable.address_bytes(), 0x01);
        let _ = self.send_command(&cmd);
        Ok(())
    }

    fn do_stop_scan(&mut self) -> bool {
        if !self.core.is_initialized() {
            return false;
        }
        let cmd = protocol::build_set_setting(Register::ScanEnable.address_bytes(), 0x00);
        self.send_command(&cmd).is_some()
    }

    fn do_read_scan(&mut self) -> Option<Vec<u8>> {
        if !self.core.is_initialized() {
            return None;
        }

        let mut attempts = 0u32;
        let max_attempts = 500_000u32;

        while attempts < max_attempts {
            match self.uart.read() {
                Ok(b) => match self.core.handle_scan_byte(b) {
                    ScanByteResult::Complete(data) => return Some(data),
                    ScanByteResult::BufferOverflow => return None,
                    ScanByteResult::NeedMore => attempts = 0,
                },
                Err(nb::Error::WouldBlock) => {
                    attempts += 1;
                }
                Err(_) => {
                    self.core.fail(ScannerError::UartError);
                    return None;
                }
            }
        }

        self.core.fail(ScannerError::Timeout);
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
        self.core.state()
    }

    fn status(&self) -> ScannerStatus {
        self.core.status()
    }

    fn data_ready(&self) -> bool {
        self.core.data_ready()
    }
}

// ============================================================================
// HIL Tests (Hardware-In-the-Loop)
// ============================================================================

#[cfg(feature = "hil-tests")]
pub mod hil_tests {
    use super::*;
    use crate::scanner_core::HilTestResults;

    pub fn run_hil_tests<UART>(scanner: &mut Gm65Scanner<UART>) -> HilTestResults
    where
        UART: embedded_hal_02::serial::Write<u8> + embedded_hal_02::serial::Read<u8>,
    {
        defmt::info!("==== HIL TESTS (SYNC) STARTING ====");

        let mut results = HilTestResults {
            init_detects_scanner: false,
            ping_after_init: false,
            trigger_and_stop: false,
            read_scan_timeout: false,
            state_transitions: false,
        };

        defmt::info!("HIL (SYNC): test_init_detects_scanner");
        results.init_detects_scanner = test_init(scanner);
        defmt::info!(
            "HIL (SYNC): {} - init_detects_scanner",
            if results.init_detects_scanner {
                "PASS"
            } else {
                "FAIL"
            }
        );

        if !results.init_detects_scanner {
            defmt::warn!("HIL (SYNC): Aborting remaining tests - no scanner detected");
            return results;
        }

        defmt::info!("HIL (SYNC): test_ping_after_init");
        results.ping_after_init = test_ping(scanner);
        defmt::info!(
            "HIL (SYNC): {} - ping_after_init",
            if results.ping_after_init {
                "PASS"
            } else {
                "FAIL"
            }
        );

        defmt::info!("HIL (SYNC): test_trigger_and_stop");
        results.trigger_and_stop = test_trigger_stop(scanner);
        defmt::info!(
            "HIL (SYNC): {} - trigger_and_stop",
            if results.trigger_and_stop {
                "PASS"
            } else {
                "FAIL"
            }
        );

        defmt::info!("HIL (SYNC): test_read_scan_timeout");
        results.read_scan_timeout = test_read_scan_timeout(scanner);
        defmt::info!(
            "HIL (SYNC): {} - read_scan_timeout",
            if results.read_scan_timeout {
                "PASS"
            } else {
                "FAIL"
            }
        );

        defmt::info!("HIL (SYNC): test_state_transitions");
        results.state_transitions = test_state_transitions(scanner);
        defmt::info!(
            "HIL (SYNC): {} - state_transitions",
            if results.state_transitions {
                "PASS"
            } else {
                "FAIL"
            }
        );

        defmt::info!(
            "==== HIL TESTS (SYNC) COMPLETE: {}/5 passed ====",
            results.passed_count()
        );

        results
    }

    fn test_init<UART>(scanner: &mut Gm65Scanner<UART>) -> bool
    where
        UART: embedded_hal_02::serial::Write<u8> + embedded_hal_02::serial::Read<u8>,
    {
        match scanner.init() {
            Ok(model) => {
                defmt::info!("HIL (SYNC): detected model = {:?}", model);
                true
            }
            Err(e) => {
                defmt::error!("HIL (SYNC): init failed with {:?}", e);
                false
            }
        }
    }

    fn test_ping<UART>(scanner: &mut Gm65Scanner<UART>) -> bool
    where
        UART: embedded_hal_02::serial::Write<u8> + embedded_hal_02::serial::Read<u8>,
    {
        let result = scanner.ping();
        if !result {
            defmt::warn!("HIL (SYNC): ping returned false");
        }
        result
    }

    fn test_trigger_stop<UART>(scanner: &mut Gm65Scanner<UART>) -> bool
    where
        UART: embedded_hal_02::serial::Write<u8> + embedded_hal_02::serial::Read<u8>,
    {
        if scanner.trigger_scan().is_err() {
            defmt::error!("HIL (SYNC): trigger_scan failed");
            return false;
        }

        if !matches!(scanner.state(), ScannerState::Scanning) {
            defmt::error!("HIL (SYNC): state not Scanning after trigger");
            return false;
        }

        if !scanner.stop_scan() {
            defmt::error!("HIL (SYNC): stop_scan failed");
            return false;
        }

        true
    }

    fn test_read_scan_timeout<UART>(scanner: &mut Gm65Scanner<UART>) -> bool
    where
        UART: embedded_hal_02::serial::Write<u8> + embedded_hal_02::serial::Read<u8>,
    {
        if scanner.trigger_scan().is_err() {
            defmt::error!("HIL (SYNC): trigger_scan failed in timeout test");
            return false;
        }

        let result = scanner.read_scan();
        let pass = result.is_none()
            && matches!(scanner.state(), ScannerState::Error(ScannerError::Timeout));

        let _ = scanner.stop_scan();

        if !pass {
            defmt::warn!("HIL (SYNC): read_scan did not timeout as expected");
        }
        pass
    }

    fn test_state_transitions<UART>(scanner: &mut Gm65Scanner<UART>) -> bool
    where
        UART: embedded_hal_02::serial::Write<u8> + embedded_hal_02::serial::Read<u8>,
    {
        let initial_state = scanner.state();
        defmt::info!("HIL (SYNC): initial state = {:?}", initial_state);

        match scanner.init() {
            Ok(_) => {
                let final_state = scanner.state();
                defmt::info!("HIL (SYNC): final state = {:?}", final_state);
                matches!(final_state, ScannerState::Ready)
            }
            Err(e) => {
                defmt::error!("HIL (SYNC): re-init failed: {:?}", e);
                false
            }
        }
    }

    pub fn run_hil_test_with_qr<UART>(scanner: &mut Gm65Scanner<UART>) -> bool
    where
        UART: embedded_hal_02::serial::Write<u8> + embedded_hal_02::serial::Read<u8>,
    {
        defmt::info!("==== HIL TEST (SYNC): SCAN WITH QR ====");
        defmt::info!("HIL (SYNC): Present QR code within 5 seconds...");

        if scanner.trigger_scan().is_err() {
            defmt::error!("HIL (SYNC): trigger failed");
            return false;
        }

        let result = scanner.read_scan();

        match result {
            Some(payload) => {
                defmt::info!("HIL (SYNC): PASS - scanned {} bytes", payload.len());
                true
            }
            None => {
                defmt::error!("HIL (SYNC): FAIL - no scan data");
                false
            }
        }
    }
}
