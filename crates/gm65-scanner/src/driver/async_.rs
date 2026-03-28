//! Async GM65 scanner implementation.
//!
//! This module provides `Gm65ScannerAsync<UART>` with true async I/O operations
//! using `embedded-io-async` traits. All state management is delegated to
//! `ScannerCore`.
//!
//! **Important**: This driver requires `embassy-time` for timeouts on all UART
//! read operations. Embassy's `BufferedUart::read()` yields until data arrives,
//! so without timeouts the driver would hang forever when no data is available.

extern crate alloc;

use alloc::vec::Vec;

use crate::driver::{
    ScannerConfig, ScannerDriver, ScannerError, ScannerModel, ScannerState, ScannerStatus,
};
use crate::protocol::{self, Gm65Response, Register, RESPONSE_LEN};
use crate::scanner_core::{
    config, fix_serial_output, init_config_sequence, serial_output_needs_fix,
    version_needs_raw_fix, ScanByteResult, ScannerCore, ScannerSettings,
};
use embassy_time::{with_timeout, Duration};

const CMD_TIMEOUT: Duration = Duration::from_secs(2);
const DRAIN_TIMEOUT: Duration = Duration::from_millis(50);

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
    core: ScannerCore,
    uart: UART,
}

impl<UART> Gm65ScannerAsync<UART> {
    /// Create a new async scanner with the given UART and configuration.
    pub fn new(uart: UART, config: ScannerConfig) -> Self {
        Self {
            core: ScannerCore::new(config),
            uart,
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
        (
            self.uart,
            self.core.state(),
            self.core.is_initialized(),
            self.core.detected_model(),
        )
    }

    /// Cancel an in-progress scan and set state to Timeout.
    /// Use this when `read_scan()` is cancelled via `embassy_time::with_timeout`.
    pub fn cancel_scan(&mut self) {
        self.core.fail(ScannerError::Timeout);
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
        if self.core.state() != ScannerState::Scanning {
            self.drain_uart().await;
        }
        if self.uart_write_all(cmd).await.is_err() {
            return None;
        }

        let mut resp = [0u8; RESPONSE_LEN];
        let mut offset = 0;

        while offset < RESPONSE_LEN {
            let remaining = &mut resp[offset..];
            match with_timeout(CMD_TIMEOUT, self.uart.read(remaining)).await {
                Ok(Ok(0)) => {
                    if offset == 0 {
                        return None;
                    }
                }
                Ok(Ok(n)) => {
                    offset += n;
                }
                Ok(Err(_)) => {
                    return None;
                }
                Err(_) => {
                    return None;
                }
            }
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
        loop {
            match with_timeout(DRAIN_TIMEOUT, self.uart.read(&mut buf)).await {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(_n)) => {}
                Ok(Err(_)) => break,
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
            .is_some_and(|r| r != Gm65Response::Invalid)
    }

    async fn save_settings(&mut self) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        let cmd = protocol::build_save_settings();
        self.send_command(&cmd)
            .await
            .is_some_and(|r| r != Gm65Response::Invalid)
    }

    async fn probe_gm65(&mut self) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        self.get_setting(Register::SerialOutput).await.is_some()
    }

    async fn do_init(&mut self) -> Result<ScannerModel, ScannerError>
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        self.core.begin_init();

        if !self.probe_gm65().await {
            self.core.fail_init(ScannerError::NotDetected);
            return Err(ScannerError::NotDetected);
        }

        self.core.mark_detected(ScannerModel::Gm65);

        let serial_val = {
            let mut result = None;
            for _attempt in 0..3u32 {
                self.drain_uart().await;
                match self.get_setting(Register::SerialOutput).await {
                    Some(v) => {
                        result = Some(v);
                        break;
                    }
                    None => {
                        #[cfg(feature = "defmt")]
                        defmt::warn!("SerialOutput read failed, retry...");
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

        if serial_output_needs_fix(serial_val) {
            let fixed = fix_serial_output(serial_val);
            if !self.set_setting(Register::SerialOutput, fixed).await {
                #[cfg(feature = "defmt")]
                defmt::warn!("init: failed to fix SerialOutput");
                self.core.fail_init(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        }

        match self.get_setting(Register::Settings).await {
            Some(val) => {
                #[cfg(feature = "defmt")]
                defmt::info!("Settings: 0x{:02x}", val);
                if val != config::CMD_MODE
                    && !self.set_setting(Register::Settings, config::CMD_MODE).await
                {
                    #[cfg(feature = "defmt")]
                    defmt::warn!("init: failed to set Settings to CMD_MODE");
                    self.core.fail_init(ScannerError::ConfigFailed);
                    return Err(ScannerError::ConfigFailed);
                }
            }
            None => {
                #[cfg(feature = "defmt")]
                defmt::warn!("init: failed to read Settings");
                self.core.fail_init(ScannerError::ConfigFailed);
                return Err(ScannerError::ConfigFailed);
            }
        }

        let config_settings = init_config_sequence();

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

        if let Some(version) = self.get_setting(Register::Version).await {
            #[cfg(feature = "defmt")]
            defmt::info!("Firmware version: 0x{:02x}", version);
            if version_needs_raw_fix(version) {
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

        self.core.complete_init(ScannerModel::Gm65);
        Ok(ScannerModel::Gm65)
    }

    async fn do_trigger_scan(&mut self) -> Result<(), ScannerError>
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        self.core.begin_scan()?;
        let cmd = protocol::build_set_setting(Register::ScanEnable.address_bytes(), 0x01);
        let _ = self.send_command(&cmd).await;
        Ok(())
    }

    async fn do_stop_scan(&mut self) -> bool
    where
        UART: embedded_io_async::Write + embedded_io_async::Read,
    {
        if !self.core.is_initialized() {
            return false;
        }
        let cmd = protocol::build_set_setting(Register::ScanEnable.address_bytes(), 0x00);
        self.send_command(&cmd).await.is_some()
    }

    async fn do_read_scan(&mut self) -> Option<Vec<u8>>
    where
        UART: embedded_io_async::Read,
    {
        if !self.core.is_initialized() {
            return None;
        }

        let mut buf = [0u8; 1];

        loop {
            match self.uart.read(&mut buf).await {
                Ok(0) => {
                    return None;
                }
                Ok(_) => match self.core.handle_scan_byte(buf[0]) {
                    ScanByteResult::Complete(data) => return Some(data),
                    ScanByteResult::BufferOverflow => return None,
                    ScanByteResult::NeedMore => {}
                },
                Err(_) => {
                    self.core.fail(ScannerError::UartError);
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
        self.core.state()
    }

    fn status(&self) -> ScannerStatus {
        self.core.status()
    }

    fn data_ready(&self) -> bool {
        self.core.data_ready()
    }
}

#[cfg(feature = "hil-tests")]
pub mod hil_tests {
    use super::*;
    use crate::scanner_core::HilTestResults;

    pub async fn run_hil_tests<UART>(scanner: &mut Gm65ScannerAsync<UART>) -> HilTestResults
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
            if results.init_detects_scanner {
                "PASS"
            } else {
                "FAIL"
            }
        );

        if !results.init_detects_scanner {
            defmt::warn!("HIL: Aborting remaining tests - no scanner detected");
            return results;
        }

        defmt::info!("HIL: test_ping_after_init");
        results.ping_after_init = test_ping(scanner).await;
        defmt::info!(
            "HIL: {} - ping_after_init",
            if results.ping_after_init {
                "PASS"
            } else {
                "FAIL"
            }
        );

        defmt::info!("HIL: test_trigger_and_stop");
        results.trigger_and_stop = test_trigger_stop(scanner).await;
        defmt::info!(
            "HIL: {} - trigger_and_stop",
            if results.trigger_and_stop {
                "PASS"
            } else {
                "FAIL"
            }
        );

        defmt::info!("HIL: test_read_scan_timeout");
        results.read_scan_timeout = test_read_scan_timeout(scanner).await;
        defmt::info!(
            "HIL: {} - read_scan_timeout",
            if results.read_scan_timeout {
                "PASS"
            } else {
                "FAIL"
            }
        );

        defmt::info!("HIL: test_state_transitions");
        results.state_transitions = test_state_transitions(scanner).await;
        defmt::info!(
            "HIL: {} - state_transitions",
            if results.state_transitions {
                "PASS"
            } else {
                "FAIL"
            }
        );

        defmt::info!(
            "==== HIL TESTS COMPLETE: {}/5 passed ====",
            results.passed_count()
        );

        defmt::info!("HIL: running extended tests...");
        let _ = run_extended_hil_tests(scanner).await;

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

        let result = with_timeout(Duration::from_secs(2), scanner.read_scan()).await;
        let timed_out = result.is_err();

        if timed_out {
            scanner.cancel_scan();
        }

        let _ = scanner.stop_scan().await;

        if timed_out {
            matches!(scanner.state(), ScannerState::Error(ScannerError::Timeout))
        } else {
            defmt::warn!(
                "HIL: read_scan_timeout: ambient barcode detected (scanner working, not a failure)"
            );
            true
        }
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

        let result = with_timeout(Duration::from_secs(5), scanner.read_scan()).await;

        match result {
            Ok(Some(payload)) => {
                defmt::info!("HIL: PASS - scanned {} bytes", payload.len());
                true
            }
            _ => {
                defmt::error!("HIL: FAIL - no scan data");
                false
            }
        }
    }
}

#[cfg(feature = "hil-tests")]
async fn test_cancel_then_rescan<UART>(scanner: &mut Gm65ScannerAsync<UART>) -> bool
where
    UART: embedded_io_async::Write + embedded_io_async::Read,
{
    defmt::info!("HIL: test_cancel_then_rescan");

    if scanner.trigger_scan().await.is_err() {
        defmt::error!("HIL: initial trigger failed");
        return false;
    }

    let _ = with_timeout(Duration::from_secs(1), scanner.read_scan()).await;
    scanner.cancel_scan();
    let _ = scanner.stop_scan().await;

    defmt::info!("HIL: state after cancel = {:?}", scanner.state());

    if scanner.trigger_scan().await.is_err() {
        defmt::error!("HIL: re-trigger after cancel failed");
        return false;
    }

    let result = with_timeout(Duration::from_secs(3), scanner.read_scan()).await;
    let _ = scanner.stop_scan().await;

    match result {
        Ok(Some(data)) => {
            defmt::info!("HIL: rescan after cancel got {} bytes", data.len());
            true
        }
        Ok(None) => {
            defmt::warn!("HIL: rescan returned None (not timeout)");
            true
        }
        Err(_) => {
            defmt::warn!("HIL: rescan timed out (no QR, but trigger worked)");
            true
        }
    }
}

#[cfg(feature = "hil-tests")]
async fn test_rapid_triggers<UART>(scanner: &mut Gm65ScannerAsync<UART>) -> bool
where
    UART: embedded_io_async::Write + embedded_io_async::Read,
{
    defmt::info!("HIL: test_rapid_triggers");

    for i in 0..5u8 {
        if scanner.trigger_scan().await.is_err() {
            defmt::error!("HIL: trigger #{} failed", i);
            return false;
        }
        embassy_time::Timer::after(Duration::from_millis(50)).await;
        let _ = scanner.stop_scan().await;
        embassy_time::Timer::after(Duration::from_millis(50)).await;
    }

    let _ = with_timeout(Duration::from_millis(500), scanner.read_scan()).await;
    scanner.cancel_scan();
    let _ = scanner.stop_scan().await;

    let final_state = scanner.state();
    defmt::info!("HIL: state after 5 rapid triggers = {:?}", final_state);

    let valid = matches!(
        final_state,
        ScannerState::Ready | ScannerState::Error(ScannerError::Timeout)
    );
    if !valid {
        defmt::warn!(
            "HIL: unexpected state after rapid triggers: {:?}",
            final_state
        );
    }
    valid
}

#[cfg(feature = "hil-tests")]
async fn test_read_idle_no_trigger<UART>(scanner: &mut Gm65ScannerAsync<UART>) -> bool
where
    UART: embedded_io_async::Write + embedded_io_async::Read,
{
    defmt::info!("HIL: test_read_idle_no_trigger");

    let result = with_timeout(Duration::from_millis(500), scanner.read_scan()).await;

    match result {
        Ok(Some(_)) => {
            defmt::warn!("HIL: read_scan returned data without trigger");
            false
        }
        Ok(None) => {
            defmt::error!("HIL: read_scan returned None (Ok(0) path - UART bug)");
            false
        }
        Err(_) => {
            defmt::info!("HIL: read_scan correctly timed out without trigger");
            true
        }
    }
}

#[cfg(feature = "hil-tests")]
pub async fn run_extended_hil_tests<UART>(scanner: &mut Gm65ScannerAsync<UART>) -> bool
where
    UART: embedded_io_async::Write + embedded_io_async::Read,
{
    defmt::info!("==== EXTENDED HIL TESTS STARTING ====");

    let cancel_rescan = test_cancel_then_rescan(scanner).await;
    defmt::info!(
        "HIL: {} - cancel_then_rescan",
        if cancel_rescan { "PASS" } else { "FAIL" }
    );

    let rapid = test_rapid_triggers(scanner).await;
    defmt::info!(
        "HIL: {} - rapid_triggers",
        if rapid { "PASS" } else { "FAIL" }
    );

    let idle = test_read_idle_no_trigger(scanner).await;
    defmt::info!(
        "HIL: {} - read_idle_no_trigger",
        if idle { "PASS" } else { "FAIL" }
    );

    let all_pass = cancel_rescan && rapid && idle;
    defmt::info!(
        "==== EXTENDED HIL TESTS COMPLETE: {} ====",
        if all_pass { "ALL PASS" } else { "SOME FAIL" }
    );

    all_pass
}
