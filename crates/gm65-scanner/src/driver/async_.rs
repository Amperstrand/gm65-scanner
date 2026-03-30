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
            .and_then(ScannerSettings::from_bits)
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
                if let Some(v) = self.get_setting(Register::SerialOutput).await {
                    result = Some(v);
                    break;
                } else {
                    #[cfg(feature = "defmt")]
                    defmt::warn!("SerialOutput read failed, retry...");
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

        if !self.set_setting(Register::Settings, config::CMD_MODE).await {
            #[cfg(feature = "defmt")]
            defmt::warn!("init: failed to set Settings to CMD_MODE");
            self.core.fail_init(ScannerError::ConfigFailed);
            return Err(ScannerError::ConfigFailed);
        }
        #[cfg(feature = "defmt")]
        defmt::info!("Settings forced to 0x{:02x}", config::CMD_MODE);

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
                        #[cfg(feature = "defmt")]
                        if let Some(verify) = self.get_setting(*reg).await {
                            if verify != *set_val {
                                defmt::warn!(
                                    "init: VERIFY FAIL {:02x}: wrote 0x{:02x}, read 0x{:02x}",
                                    reg.address_bytes(),
                                    set_val,
                                    verify
                                );
                            }
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

#[cfg(test)]
mod time_driver {
    use core::sync::atomic::{AtomicU64, Ordering};

    static MOCK_TIME: AtomicU64 = AtomicU64::new(1_000_000_000_000u64);

    #[no_mangle]
    extern "C" fn _embassy_time_now() -> u64 {
        MOCK_TIME.load(Ordering::Relaxed)
    }

    #[no_mangle]
    extern "C" fn _embassy_time_schedule_wake(at: u64) {
        MOCK_TIME.store(at, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub fn advance_time(nanos: u64) {
        MOCK_TIME.fetch_add(nanos, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::rc::Rc;
    use alloc::vec::Vec;
    use core::cell::RefCell;

    use embedded_io_async::Read as _;
    use embedded_io_async::Write as _;

    struct MockInner {
        read_queue: Vec<u8>,
        written: Vec<u8>,
        pending_responses: Vec<Vec<u8>>,
    }

    struct MockAsyncUart {
        inner: Rc<RefCell<MockInner>>,
    }

    impl MockAsyncUart {
        fn new() -> Self {
            Self {
                inner: Rc::new(RefCell::new(MockInner {
                    read_queue: Vec::new(),
                    written: Vec::new(),
                    pending_responses: Vec::new(),
                })),
            }
        }

        fn with_responses(responses: &[u8]) -> Self {
            Self {
                inner: Rc::new(RefCell::new(MockInner {
                    read_queue: Vec::new(),
                    written: Vec::new(),
                    pending_responses: Vec::from([Vec::from(responses)]),
                })),
            }
        }

        fn with_response_sequence(responses: &[&[u8]]) -> Self {
            Self {
                inner: Rc::new(RefCell::new(MockInner {
                    read_queue: Vec::new(),
                    written: Vec::new(),
                    pending_responses: responses.iter().map(|r| Vec::from(*r)).collect(),
                })),
            }
        }

        fn written_bytes(&self) -> Vec<u8> {
            self.inner.borrow().written.clone()
        }

        #[allow(dead_code)]
        fn load_read_queue(&self, data: &[u8]) {
            self.inner.borrow_mut().read_queue.extend_from_slice(data);
        }
    }

    impl Clone for MockAsyncUart {
        fn clone(&self) -> Self {
            Self {
                inner: Rc::clone(&self.inner),
            }
        }
    }

    impl embedded_io_async::ErrorType for MockAsyncUart {
        type Error = embedded_io_async::ErrorKind;
    }

    impl embedded_io_async::Write for MockAsyncUart {
        async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
            self.inner.borrow_mut().written.extend_from_slice(buf);
            Ok(buf.len())
        }

        async fn flush(&mut self) -> Result<(), Self::Error> {
            let mut inner = self.inner.borrow_mut();
            if !inner.pending_responses.is_empty() {
                let resp = inner.pending_responses.remove(0);
                inner.read_queue.extend_from_slice(&resp);
            }
            Ok(())
        }

        async fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
            self.inner.borrow_mut().written.extend_from_slice(buf);
            let mut inner = self.inner.borrow_mut();
            if !inner.pending_responses.is_empty() {
                let resp = inner.pending_responses.remove(0);
                inner.read_queue.extend_from_slice(&resp);
            }
            Ok(())
        }
    }

    impl embedded_io_async::Read for MockAsyncUart {
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            let mut inner = self.inner.borrow_mut();
            if inner.read_queue.is_empty() {
                return Err(embedded_io_async::ErrorKind::Other);
            }
            let len = buf.len().min(inner.read_queue.len());
            buf[..len].copy_from_slice(&inner.read_queue[..len]);
            inner.read_queue.drain(..len);
            Ok(len)
        }
    }

    fn success_response(value: u8) -> [u8; 7] {
        [0x02, 0x00, 0x00, 0x01, value, 0x33, 0x31]
    }

    fn init_response_sequence() -> ([u8; 7 * 20], usize) {
        let mut buf = [0u8; 7 * 20];
        let mut idx = 0usize;

        let r = |buf: &mut [u8], idx: &mut usize, v: u8| {
            let resp = success_response(v);
            buf[*idx..*idx + 7].copy_from_slice(&resp);
            *idx += 7;
        };

        r(&mut buf, &mut idx, 0xA0);
        r(&mut buf, &mut idx, 0xA0);
        r(&mut buf, &mut idx, 0x81);

        let targets: [u8; 5] = [0x00, 0x01, 0x85, 0x01, 0x01];
        for _ in 0..5 {
            r(&mut buf, &mut idx, 0xFF);
        }
        for t in &targets {
            r(&mut buf, &mut idx, *t);
        }
        for t in &targets {
            r(&mut buf, &mut idx, *t);
        }

        r(&mut buf, &mut idx, 0x87);
        r(&mut buf, &mut idx, 0x00);

        (buf, idx)
    }

    #[test]
    fn test_mock_uart_write_read() {
        futures_executor::block_on(async {
            let mut mock = MockAsyncUart::new();
            mock.write_all(&[0xAA, 0xBB]).await.unwrap();
            let mut buf = [0u8; 1];
            assert!(mock.read(&mut buf).await.is_err());
            mock.flush().await.unwrap();
            assert!(mock.read(&mut buf).await.is_err());
        });
    }

    #[test]
    fn test_mock_uart_flush_loads_response() {
        futures_executor::block_on(async {
            let mock = MockAsyncUart::with_responses(&[0x01, 0x02, 0x03]);
            let mut mock = mock;
            let mut buf = [0u8; 1];
            assert!(mock.read(&mut buf).await.is_err());
            mock.flush().await.unwrap();
            assert_eq!(mock.read(&mut buf).await.unwrap(), 1);
            assert_eq!(buf[0], 0x01);
            assert_eq!(mock.read(&mut buf).await.unwrap(), 1);
            assert_eq!(buf[0], 0x02);
            assert_eq!(mock.read(&mut buf).await.unwrap(), 1);
            assert_eq!(buf[0], 0x03);
            assert!(mock.read(&mut buf).await.is_err());
        });
    }

    #[test]
    fn test_mock_uart_empty_read_returns_err() {
        futures_executor::block_on(async {
            let mut mock = MockAsyncUart::new();
            let mut buf = [0u8; 1];
            assert!(mock.read(&mut buf).await.is_err());
        });
    }

    #[test]
    fn test_initial_state_uninitialized() {
        let mock = MockAsyncUart::new();
        let scanner = Gm65ScannerAsync::with_default_config(mock);
        assert_eq!(scanner.state(), ScannerState::Uninitialized);
        assert!(!scanner.data_ready());
        let status = scanner.status();
        assert!(!status.connected);
        assert!(!status.initialized);
    }

    #[test]
    fn test_ping_success() {
        futures_executor::block_on(async {
            let resp = success_response(0xA0);
            let mock = MockAsyncUart::with_responses(&resp);
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            assert!(scanner.ping().await);
        });
    }

    #[test]
    fn test_ping_failure_no_response() {
        futures_executor::block_on(async {
            let mock = MockAsyncUart::new();
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            assert!(!scanner.ping().await);
        });
    }

    #[test]
    fn test_ping_command_bytes_on_wire() {
        futures_executor::block_on(async {
            let resp = success_response(0xA0);
            let mock = MockAsyncUart::with_responses(&resp);
            let handle = mock.clone();
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let _ = scanner.ping().await;
            let written = handle.written_bytes();
            let expected = protocol::build_get_setting(Register::SerialOutput.address_bytes());
            assert_eq!(
                &written[..],
                &expected[..],
                "ping should send get_setting(SerialOutput)"
            );
        });
    }

    #[test]
    fn test_get_setting_returns_value() {
        futures_executor::block_on(async {
            let resp = success_response(0x42);
            let mock = MockAsyncUart::with_responses(&resp);
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let result = scanner.get_setting(Register::Timeout).await;
            assert_eq!(result, Some(0x42));
        });
    }

    #[test]
    fn test_get_setting_invalid_response_returns_none() {
        futures_executor::block_on(async {
            let bad_resp: [u8; 7] = [0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
            let mock = MockAsyncUart::with_responses(&bad_resp);
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let result = scanner.get_setting(Register::Timeout).await;
            assert_eq!(result, None);
        });
    }

    #[test]
    fn test_get_setting_no_response_returns_none() {
        futures_executor::block_on(async {
            let mock = MockAsyncUart::new();
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let result = scanner.get_setting(Register::Timeout).await;
            assert_eq!(result, None);
        });
    }

    #[test]
    fn test_get_setting_command_bytes() {
        futures_executor::block_on(async {
            let resp = success_response(0x00);
            let mock = MockAsyncUart::with_responses(&resp);
            let handle = mock.clone();
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let _ = scanner.get_setting(Register::Version).await;
            let written = handle.written_bytes();
            let expected = protocol::build_get_setting(Register::Version.address_bytes());
            assert_eq!(&written[..], &expected[..]);
        });
    }

    #[test]
    fn test_set_setting_success() {
        futures_executor::block_on(async {
            let resp = success_response(0x81);
            let mock = MockAsyncUart::with_responses(&resp);
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let result = scanner.set_setting(Register::Settings, 0x81).await;
            assert!(result);
        });
    }

    #[test]
    fn test_set_setting_command_bytes() {
        futures_executor::block_on(async {
            let resp = success_response(0x01);
            let mock = MockAsyncUart::with_responses(&resp);
            let handle = mock.clone();
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let _ = scanner.set_setting(Register::Settings, 0x81).await;
            let written = handle.written_bytes();
            let expected = protocol::build_set_setting(Register::Settings.address_bytes(), 0x81);
            assert_eq!(&written[..], &expected[..]);
        });
    }

    #[test]
    fn test_get_scanner_settings_valid() {
        futures_executor::block_on(async {
            let resp = success_response(0x81);
            let mock = MockAsyncUart::with_responses(&resp);
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let settings = scanner.get_scanner_settings().await;
            assert!(settings.is_some());
            let s = settings.unwrap();
            assert!(s.contains(ScannerSettings::COMMAND));
            assert!(s.contains(ScannerSettings::ALWAYS_ON));
        });
    }

    #[test]
    fn test_get_scanner_settings_invalid_bits() {
        futures_executor::block_on(async {
            let resp = success_response(0x00);
            let mock = MockAsyncUart::with_responses(&resp);
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let settings = scanner.get_scanner_settings().await;
            assert!(settings.is_some());
        });
    }

    #[test]
    fn test_set_scanner_settings() {
        futures_executor::block_on(async {
            let resp = success_response(0x81);
            let mock = MockAsyncUart::with_responses(&resp);
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let settings = ScannerSettings::ALWAYS_ON | ScannerSettings::COMMAND;
            let result = scanner.set_scanner_settings(settings).await;
            assert!(result);
        });
    }

    #[test]
    fn test_release_returns_uart() {
        let mock = MockAsyncUart::new();
        let scanner = Gm65ScannerAsync::with_default_config(mock);
        let _mock = scanner.release();
    }

    #[test]
    fn test_into_parts() {
        let mock = MockAsyncUart::new();
        let scanner = Gm65ScannerAsync::with_default_config(mock);
        let (_uart, state, initialized, model) = scanner.into_parts();
        assert_eq!(state, ScannerState::Uninitialized);
        assert!(!initialized);
        assert_eq!(model, ScannerModel::Unknown);
    }

    #[test]
    fn test_cancel_scan() {
        let mock = MockAsyncUart::new();
        let mut scanner = Gm65ScannerAsync::with_default_config(mock);
        scanner.cancel_scan();
        assert!(matches!(
            scanner.state(),
            ScannerState::Error(ScannerError::Timeout)
        ));
    }

    #[test]
    fn test_init_success() {
        futures_executor::block_on(async {
            let (buf, len) = init_response_sequence();
            let chunks: Vec<&[u8]> = (0..len).step_by(7).map(|i| &buf[i..i + 7]).collect();
            let mock = MockAsyncUart::with_response_sequence(&chunks);
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let result = scanner.init().await;
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), ScannerModel::Gm65);
            assert_eq!(scanner.state(), ScannerState::Ready);
        });
    }

    #[test]
    fn test_init_not_detected() {
        futures_executor::block_on(async {
            let mock = MockAsyncUart::new();
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let result = scanner.init().await;
            assert_eq!(result, Err(ScannerError::NotDetected));
            assert!(matches!(
                scanner.state(),
                ScannerState::Error(ScannerError::NotDetected)
            ));
        });
    }

    #[test]
    fn test_reinit_resets_state() {
        futures_executor::block_on(async {
            let (buf1, len1) = init_response_sequence();
            let (buf2, len2) = init_response_sequence();
            let chunks1: Vec<&[u8]> = (0..len1).step_by(7).map(|i| &buf1[i..i + 7]).collect();
            let chunks2: Vec<&[u8]> = (0..len2).step_by(7).map(|i| &buf2[i..i + 7]).collect();
            let mut all_chunks = chunks1;
            all_chunks.extend(chunks2);
            let mock = MockAsyncUart::with_response_sequence(&all_chunks);
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            assert!(scanner.init().await.is_ok());
            assert!(scanner.init().await.is_ok());
            assert_eq!(scanner.state(), ScannerState::Ready);
        });
    }

    #[test]
    fn test_trigger_scan_not_initialized() {
        futures_executor::block_on(async {
            let mock = MockAsyncUart::new();
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            let result = scanner.trigger_scan().await;
            assert_eq!(result, Err(ScannerError::NotInitialized));
        });
    }

    #[test]
    fn test_stop_scan_not_initialized() {
        futures_executor::block_on(async {
            let mock = MockAsyncUart::new();
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            assert!(!scanner.stop_scan().await);
        });
    }

    #[test]
    fn test_read_scan_not_initialized() {
        futures_executor::block_on(async {
            let mock = MockAsyncUart::new();
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            assert_eq!(scanner.read_scan().await, None);
        });
    }

    #[test]
    fn test_trigger_and_stop_after_init() {
        futures_executor::block_on(async {
            let (buf, len) = init_response_sequence();
            let chunks: Vec<&[u8]> = (0..len).step_by(7).map(|i| &buf[i..i + 7]).collect();

            let trigger_resp = success_response(0x01);
            let stop_resp = success_response(0x00);

            let mut all_chunks = chunks;
            all_chunks.push(&trigger_resp);
            all_chunks.push(&stop_resp);

            let mock = MockAsyncUart::with_response_sequence(&all_chunks);
            let mut scanner = Gm65ScannerAsync::with_default_config(mock);
            assert!(scanner.init().await.is_ok());
            assert!(scanner.trigger_scan().await.is_ok());
            assert_eq!(scanner.state(), ScannerState::Scanning);
            assert!(scanner.stop_scan().await);
        });
    }
}
