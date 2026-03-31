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
use crate::scanner_core::{ScanByteResult, ScannerCore, ScannerSettings};

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
            .and_then(ScannerSettings::from_bits)
    }

    /// Set scanner settings from bitflags.
    pub fn set_scanner_settings(&mut self, settings: ScannerSettings) -> bool {
        self.set_setting(Register::Settings, settings.bits())
    }

    /// Poll UART for a single byte (non-blocking).
    #[must_use]
    pub fn poll_uart(&mut self) -> Option<u8> {
        match self.uart.read() {
            Ok(b) => Some(b),
            Err(nb::Error::WouldBlock) => None,
            Err(_) => None,
        }
    }

    /// Non-blocking incremental scan read.
    /// Drains all available bytes from the UART FIFO in a single call.
    /// Returns Some(data) when a complete scan is received, None if UART is empty.
    #[must_use]
    pub fn try_read_scan(&mut self) -> Option<Vec<u8>> {
        if !self.core.is_initialized() {
            return None;
        }
        // Reset state if previous scan complete
        if self.core.state() == ScannerState::ScanComplete {
            self.core.begin_scan().ok();
        }
        loop {
            match self.poll_uart() {
                Some(byte) => match self.core.handle_scan_byte(byte) {
                    ScanByteResult::Complete(data) => return Some(data),
                    ScanByteResult::BufferOverflow => return None,
                    ScanByteResult::NeedMore => continue,
                },
                None => return None,
            }
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
        if self.core.state() != ScannerState::Scanning {
            self.drain_uart();
        }
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

    #[must_use]
    pub fn get_setting(&mut self, reg: Register) -> Option<u8> {
        let cmd = protocol::build_get_setting(reg.address_bytes());
        match self.send_command(&cmd) {
            Some(Gm65Response::SuccessWithValue(v)) => Some(v),
            _ => None,
        }
    }

    fn set_setting(&mut self, reg: Register, value: u8) -> bool {
        let cmd = protocol::build_set_setting(reg.address_bytes(), value);
        self.send_command(&cmd)
            .is_some_and(|r| r != Gm65Response::Invalid)
    }

    fn save_settings(&mut self) -> bool {
        let cmd = protocol::build_save_settings();
        let result = self
            .send_command(&cmd)
            .is_some_and(|r| r != Gm65Response::Invalid);
        #[cfg(feature = "defmt")]
        defmt::info!("save_settings: {}", if result { "OK" } else { "FAIL" });
        result
    }

    fn do_init(&mut self) -> Result<ScannerModel, ScannerError> {
        use crate::scanner_core::InitAction;
        let mut action = self.core.init_begin();
        loop {
            match action {
                InitAction::DrainAndRead(reg) => {
                    self.drain_uart();
                    let result = self.get_setting(reg);
                    action = self.core.init_advance(result);
                }
                InitAction::ReadRegister(reg) => {
                    let result = self.get_setting(reg);
                    action = self.core.init_advance(result);
                }
                InitAction::WriteRegister(reg, val) => {
                    let ok = self.set_setting(reg, val);
                    action = self.core.init_advance(if ok { Some(val) } else { None });
                }
                InitAction::VerifyRegister(_reg, _expected) => {
                    #[cfg(feature = "defmt")]
                    {
                        let verify = self.get_setting(_reg);
                        if let Some(verify) = verify {
                            if verify != _expected {
                                defmt::warn!(
                                    "init: VERIFY FAIL {:02x}: wrote 0x{:02x}, read 0x{:02x}",
                                    _reg.address_bytes(),
                                    _expected,
                                    verify
                                );
                            }
                        }
                    }
                    action = self.core.init_advance_verify();
                }
                InitAction::Complete(model) => {
                    let _ = self.save_settings();
                    #[cfg(feature = "defmt")]
                    defmt::info!("init: complete");
                    self.core.complete_init(model);
                    return Ok(model);
                }
                InitAction::Fail(e) => {
                    return Err(e);
                }
            }
        }
    }

    fn do_trigger_scan(&mut self) -> Result<(), ScannerError> {
        self.core.begin_scan()?;
        let cmd = protocol::build_set_setting(Register::ScanEnable.address_bytes(), 0x01);
        let _resp = self.send_command(&cmd);
        #[cfg(feature = "defmt")]
        match &_resp {
            Some(_) => defmt::info!("Trigger ScanEnable: ack ok"),
            None => defmt::warn!("Trigger ScanEnable: NO RESPONSE"),
        }
        Ok(())
    }

    fn do_stop_scan(&mut self) -> bool {
        if !self.core.is_initialized() {
            return false;
        }
        let cmd = protocol::build_set_setting(Register::ScanEnable.address_bytes(), 0x00);
        let resp = self.send_command(&cmd);
        #[cfg(feature = "defmt")]
        match &resp {
            Some(_) => defmt::info!("Stop ScanEnable: ack ok"),
            None => defmt::warn!("Stop ScanEnable: NO RESPONSE"),
        }
        resp.is_some()
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

    fn try_read_scan(&mut self) -> Option<Vec<u8>> {
        self.try_read_scan()
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
        let timed_out = result.is_none();

        let _ = scanner.stop_scan();

        if timed_out {
            matches!(scanner.state(), ScannerState::Error(ScannerError::Timeout))
        } else {
            defmt::warn!("HIL (SYNC): read_scan_timeout: ambient barcode detected (scanner working, not a failure)");
            true
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::rc::Rc;
    use alloc::vec::Vec;
    use core::cell::RefCell;

    use crate::driver::test_helpers::{init_response_sequence, success_response, MockInner};
    use embedded_hal_02::serial::Read as _;
    use embedded_hal_02::serial::Write as _;

    struct MockUart {
        inner: Rc<RefCell<MockInner>>,
    }

    impl MockUart {
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
        fn push_response(&self, data: &[u8]) {
            self.inner
                .borrow_mut()
                .pending_responses
                .push(Vec::from(data));
        }

        fn load_read_queue(&self, data: &[u8]) {
            self.inner.borrow_mut().read_queue.extend_from_slice(data);
        }
    }

    impl Clone for MockUart {
        fn clone(&self) -> Self {
            Self {
                inner: Rc::clone(&self.inner),
            }
        }
    }

    impl embedded_hal_02::serial::Write<u8> for MockUart {
        type Error = ();

        fn write(&mut self, byte: u8) -> Result<(), nb::Error<Self::Error>> {
            self.inner.borrow_mut().written.push(byte);
            Ok(())
        }

        fn flush(&mut self) -> Result<(), nb::Error<Self::Error>> {
            let mut inner = self.inner.borrow_mut();
            if !inner.pending_responses.is_empty() {
                let resp = inner.pending_responses.remove(0);
                inner.read_queue.extend_from_slice(&resp);
            }
            Ok(())
        }
    }

    impl embedded_hal_02::serial::Read<u8> for MockUart {
        type Error = ();

        fn read(&mut self) -> Result<u8, nb::Error<Self::Error>> {
            let mut inner = self.inner.borrow_mut();
            if inner.read_queue.is_empty() {
                Err(nb::Error::WouldBlock)
            } else {
                Ok(inner.read_queue.remove(0))
            }
        }
    }

    #[test]
    fn test_mock_uart_write_read() {
        let mut mock = MockUart::new();
        mock.write(0xAA).unwrap();
        mock.write(0xBB).unwrap();
        assert!(mock.read().is_err());
        mock.flush().unwrap();
        assert!(mock.read().is_err());
    }

    #[test]
    fn test_mock_uart_flush_loads_response() {
        let mock = MockUart::with_responses(&[0x01, 0x02, 0x03]);
        let mut mock = mock;
        assert!(mock.read().is_err());
        mock.flush().unwrap();
        assert_eq!(mock.read().unwrap(), 0x01);
        assert_eq!(mock.read().unwrap(), 0x02);
        assert_eq!(mock.read().unwrap(), 0x03);
        assert!(mock.read().is_err());
    }

    #[test]
    fn test_mock_uart_empty_read_returns_wouldblock() {
        let mut mock = MockUart::new();
        match mock.read() {
            Err(nb::Error::WouldBlock) => {}
            _ => panic!("expected WouldBlock"),
        }
    }

    #[test]
    fn test_initial_state_uninitialized() {
        let mock = MockUart::new();
        let scanner = Gm65Scanner::with_default_config(mock);
        assert_eq!(scanner.state(), ScannerState::Uninitialized);
        assert!(!scanner.data_ready());
        let status = scanner.status();
        assert!(!status.connected);
        assert!(!status.initialized);
    }

    #[test]
    fn test_ping_success() {
        let resp = success_response(0xA0);
        let mock = MockUart::with_responses(&resp);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        assert!(scanner.ping());
    }

    #[test]
    fn test_ping_failure_no_response() {
        let mock = MockUart::new();
        let mut scanner = Gm65Scanner::with_default_config(mock);
        assert!(!scanner.ping());
    }

    #[test]
    fn test_ping_command_bytes_on_wire() {
        let resp = success_response(0xA0);
        let mock = MockUart::with_responses(&resp);
        let handle = mock.clone();
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let _ = scanner.ping();
        let written = handle.written_bytes();
        let expected = protocol::build_get_setting(Register::SerialOutput.address_bytes());
        assert_eq!(
            &written[..],
            &expected[..],
            "ping should send get_setting(SerialOutput)"
        );
    }

    #[test]
    fn test_get_setting_returns_value() {
        let resp = success_response(0x42);
        let mock = MockUart::with_responses(&resp);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let result = scanner.get_setting(Register::Timeout);
        assert_eq!(result, Some(0x42));
    }

    #[test]
    fn test_get_setting_invalid_response_returns_none() {
        let bad_resp: [u8; 7] = [0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let mock = MockUart::with_responses(&bad_resp);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let result = scanner.get_setting(Register::Timeout);
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_setting_no_response_returns_none() {
        let mock = MockUart::new();
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let result = scanner.get_setting(Register::Timeout);
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_setting_command_bytes() {
        let resp = success_response(0x00);
        let mock = MockUart::with_responses(&resp);
        let handle = mock.clone();
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let _ = scanner.get_setting(Register::Version);
        let written = handle.written_bytes();
        let expected = protocol::build_get_setting(Register::Version.address_bytes());
        assert_eq!(&written[..], &expected[..]);
    }

    #[test]
    fn test_set_setting_success() {
        let resp = success_response(0x81);
        let mock = MockUart::with_responses(&resp);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let result = scanner.set_setting(Register::Settings, 0x81);
        assert!(result);
    }

    #[test]
    fn test_set_setting_command_bytes() {
        let resp = success_response(0x01);
        let mock = MockUart::with_responses(&resp);
        let handle = mock.clone();
        let mut scanner = Gm65Scanner::with_default_config(mock);
        scanner.set_setting(Register::Settings, 0x81);
        let written = handle.written_bytes();
        let expected = protocol::build_set_setting(Register::Settings.address_bytes(), 0x81);
        assert_eq!(&written[..], &expected[..]);
    }

    #[test]
    fn test_get_scanner_settings_valid() {
        let resp = success_response(0x81);
        let mock = MockUart::with_responses(&resp);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let settings = scanner.get_scanner_settings();
        assert!(settings.is_some());
        let s = settings.unwrap();
        assert!(s.contains(ScannerSettings::COMMAND));
        assert!(s.contains(ScannerSettings::ALWAYS_ON));
    }

    #[test]
    fn test_get_scanner_settings_invalid_bits() {
        let resp = success_response(0x00);
        let mock = MockUart::with_responses(&resp);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let settings = scanner.get_scanner_settings();
        assert!(settings.is_some());
    }

    #[test]
    fn test_set_scanner_settings() {
        let resp = success_response(0x81);
        let mock = MockUart::with_responses(&resp);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let settings = ScannerSettings::ALWAYS_ON | ScannerSettings::COMMAND;
        let result = scanner.set_scanner_settings(settings);
        assert!(result);
    }

    #[test]
    fn test_release_returns_uart() {
        let mock = MockUart::new();
        let scanner = Gm65Scanner::with_default_config(mock);
        let _mock = scanner.release();
    }

    #[test]
    fn test_into_parts() {
        let mock = MockUart::new();
        let scanner = Gm65Scanner::with_default_config(mock);
        let (_uart, state, initialized, model) = scanner.into_parts();
        assert_eq!(state, ScannerState::Uninitialized);
        assert!(!initialized);
        assert_eq!(model, ScannerModel::Unknown);
    }

    #[test]
    fn test_poll_uart_returns_byte() {
        let mock = MockUart::new();
        mock.load_read_queue(&[0xAB]);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        assert_eq!(scanner.poll_uart(), Some(0xAB));
    }

    #[test]
    fn test_poll_uart_returns_none_when_empty() {
        let mock = MockUart::new();
        let mut scanner = Gm65Scanner::with_default_config(mock);
        assert_eq!(scanner.poll_uart(), None);
    }

    #[test]
    fn test_try_read_scan_uninitialized_returns_none() {
        let mock = MockUart::with_responses(&[0x01, 0x02, 0x03]);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        assert_eq!(scanner.try_read_scan(), None);
    }

    #[test]
    fn test_init_success() {
        let (buf, len) = init_response_sequence();
        let chunks: Vec<&[u8]> = (0..len).step_by(7).map(|i| &buf[i..i + 7]).collect();
        let mock = MockUart::with_response_sequence(&chunks);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let result = scanner.init();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ScannerModel::Gm65);
        assert_eq!(scanner.state(), ScannerState::Ready);
    }

    #[test]
    fn test_init_not_detected() {
        let mock = MockUart::new();
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let result = scanner.init();
        assert_eq!(result, Err(ScannerError::NotDetected));
        assert!(matches!(
            scanner.state(),
            ScannerState::Error(ScannerError::NotDetected)
        ));
    }

    #[test]
    fn test_reinit_resets_state() {
        let (buf1, len1) = init_response_sequence();
        let (buf2, len2) = init_response_sequence();
        let chunks1: Vec<&[u8]> = (0..len1).step_by(7).map(|i| &buf1[i..i + 7]).collect();
        let chunks2: Vec<&[u8]> = (0..len2).step_by(7).map(|i| &buf2[i..i + 7]).collect();
        let mut all_chunks = chunks1;
        all_chunks.extend(chunks2);
        let mock = MockUart::with_response_sequence(&all_chunks);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        assert!(scanner.init().is_ok());
        assert!(scanner.init().is_ok());
        assert_eq!(scanner.state(), ScannerState::Ready);
    }

    #[test]
    fn test_trigger_scan_not_initialized() {
        let mock = MockUart::new();
        let mut scanner = Gm65Scanner::with_default_config(mock);
        let result = scanner.trigger_scan();
        assert_eq!(result, Err(ScannerError::NotInitialized));
    }

    #[test]
    fn test_stop_scan_not_initialized() {
        let mock = MockUart::new();
        let mut scanner = Gm65Scanner::with_default_config(mock);
        assert!(!scanner.stop_scan());
    }

    #[test]
    fn test_read_scan_not_initialized() {
        let mock = MockUart::new();
        let mut scanner = Gm65Scanner::with_default_config(mock);
        assert_eq!(scanner.read_scan(), None);
    }

    #[test]
    fn test_trigger_and_stop_after_init() {
        let (buf, len) = init_response_sequence();
        let chunks: Vec<&[u8]> = (0..len).step_by(7).map(|i| &buf[i..i + 7]).collect();

        let trigger_resp = success_response(0x01);
        let stop_resp = success_response(0x00);

        let mut all_chunks = chunks;
        all_chunks.push(&trigger_resp);
        all_chunks.push(&stop_resp);

        let mock = MockUart::with_response_sequence(&all_chunks);
        let mut scanner = Gm65Scanner::with_default_config(mock);
        assert!(scanner.init().is_ok());
        assert!(scanner.trigger_scan().is_ok());
        assert_eq!(scanner.state(), ScannerState::Scanning);
        assert!(scanner.stop_scan());
    }
}
