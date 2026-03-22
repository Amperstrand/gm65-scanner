//! Sans-IO scanner core implementation.
//!
//! Extracts shared logic from sync.rs and async_.rs into a single core
//! that manages state and buffer without I/O operations.
//!
//! This module contains:
//! - Buffer management for scan data
//! - Init sequence state machine
//! - Configuration constants and helpers
//! - ScannerSettings bitflags

extern crate alloc;

use crate::buffer::ScanBuffer;
use crate::driver::{ScannerConfig, ScannerError, ScannerModel, ScannerState, ScannerStatus};
use crate::protocol::Register;

// ============================================================================
// Configuration Constants
// ============================================================================

/// Configuration constants for GM65 scanner.
///
/// These values are used during the initialization sequence to configure
/// the scanner for command-triggered operation.
pub mod config {
    /// Scan interval in milliseconds.
    pub const SCAN_INTERVAL_MS: u8 = 0x01;

    /// Delay before scanning same barcode again.
    pub const SAME_BARCODE_DELAY: u8 = 0x85;

    /// Command mode settings value (ALWAYS_ON | SOUND | AIM | COMMAND).
    pub const CMD_MODE: u8 = 0xD1;

    /// Firmware version that requires raw mode fix.
    pub const VERSION_NEEDS_RAW: u8 = 0x69;

    /// Raw mode value for firmware fix.
    pub const RAW_MODE_VALUE: u8 = 0x08;
}

// ============================================================================
// ScannerSettings Bitflags
// ============================================================================

bitflags::bitflags! {
    /// Scanner settings bitflags for the Settings register.
    ///
    /// These flags control various scanner behaviors like always-on mode,
    /// sound feedback, aiming light, etc.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ScannerSettings: u8 {
        /// Keep scanner always on (don't sleep).
        const ALWAYS_ON  = 1 << 7;
        /// Enable beep on successful scan.
        const SOUND      = 1 << 6;
        /// Unknown bit 5 (reserved).
        const UNKNOWN_5  = 1 << 5;
        /// Enable aiming light pattern.
        const AIM        = 1 << 4;
        /// Unknown bit 3 (reserved).
        const UNKNOWN_3  = 1 << 3;
        /// Enable illumination light.
        const LIGHT      = 1 << 2;
        /// Enable continuous scanning mode.
        const CONTINUOUS = 1 << 1;
        /// Enable command-triggered mode.
        const COMMAND    = 1 << 0;
    }
}

impl Default for ScannerSettings {
    fn default() -> Self {
        Self::ALWAYS_ON | Self::SOUND | Self::AIM | Self::COMMAND
    }
}

// ============================================================================
// Init Sequence Configuration
// ============================================================================

/// Register configuration tuple: (register, expected_value).
pub type RegisterConfig = (Register, u8);

/// Returns the standard initialization register configuration sequence.
///
/// Each tuple contains a register and its target value. During initialization,
/// each register is read and only written if the current value differs from
/// the target.
///
/// # Example
///
/// ```rust,ignore
/// let config = init_config_sequence();
/// for (reg, target_val) in config.iter() {
///     let current = read_register(*reg)?;
///     if current != *target_val {
///         write_register(*reg, *target_val)?;
///     }
/// }
/// ```
pub fn init_config_sequence() -> [RegisterConfig; 5] {
    use config::*;
    [
        (Register::Timeout, 0x00),
        (Register::ScanInterval, SCAN_INTERVAL_MS),
        (Register::SameBarcodeDelay, SAME_BARCODE_DELAY),
        (Register::BarType, 0x01),
        (Register::QrEnable, 0x01),
    ]
}

/// Returns registers that require special handling during init.
///
/// These registers have logic beyond simple read-compare-write:
/// - `SerialOutput`: Bits 0-1 must be cleared for proper serial communication
/// - `Settings`: Must be set to CMD_MODE for command-triggered scanning
/// - `Version`: Checked to determine if raw mode fix is needed
pub fn special_registers() -> [Register; 3] {
    [
        Register::SerialOutput,
        Register::Settings,
        Register::Version,
    ]
}

// ============================================================================
// Serial Output Helpers
// ============================================================================

/// Check if SerialOutput value needs fixing.
///
/// The SerialOutput register should have bits 0-1 cleared (0) for proper
/// serial communication. If these bits are set, they indicate an incorrect
/// serial output mode that will cause communication issues.
///
/// # Returns
///
/// `true` if bits 0-1 are set (value needs fixing), `false` otherwise.
#[inline]
pub fn serial_output_needs_fix(value: u8) -> bool {
    value & 0x03 != 0
}

/// Fix SerialOutput value by clearing bits 0-1.
///
/// # Returns
///
/// The value with bits 0-1 cleared.
#[inline]
pub fn fix_serial_output(value: u8) -> u8 {
    value & 0xFC
}

// ============================================================================
// Version Helpers
// ============================================================================

/// Check if firmware version needs raw mode fix.
///
/// Certain firmware versions (specifically 0x69) require the RawMode register
/// to be set to a specific value for proper operation.
///
/// # Returns
///
/// `true` if the version requires the raw mode fix.
#[inline]
pub fn version_needs_raw_fix(version: u8) -> bool {
    version == config::VERSION_NEEDS_RAW
}

// ============================================================================
// Scan Byte Result
// ============================================================================

/// Result of processing a scan byte.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ScanByteResult {
    /// Need more bytes to complete scan.
    NeedMore,
    /// Complete scan data ready.
    Complete(#[cfg_attr(feature = "defmt", defmt(Debug2Format))] alloc::vec::Vec<u8>),
    /// Buffer overflow detected.
    BufferOverflow,
}

// ============================================================================
// HIL Test Results
// ============================================================================

/// Results from Hardware-In-the-Loop (HIL) tests.
#[derive(Debug, Clone, Copy)]
#[cfg(feature = "hil-tests")]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct HilTestResults {
    pub init_detects_scanner: bool,
    pub ping_after_init: bool,
    pub trigger_and_stop: bool,
    pub read_scan_timeout: bool,
    pub state_transitions: bool,
}

#[cfg(feature = "hil-tests")]
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

// ============================================================================
// Init Step Tracker
// ============================================================================

/// Initialization step tracker.
///
/// Tracks the progress of the scanner initialization sequence.
/// Used by both sync and async drivers to report init state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InitStep {
    /// Initial state, not yet started.
    #[default]
    Start,
    /// Probing for scanner presence.
    Detecting,
    /// Reading SerialOutput register.
    ReadSerialOutput,
    /// Fixing SerialOutput register.
    FixSerialOutput,
    /// Setting command mode.
    SetCommandMode,
    /// Applying configuration at given index.
    ApplyConfig {
        /// Index into init_config_sequence().
        index: usize,
    },
    /// Checking firmware version.
    CheckVersion,
    /// Saving settings to NVRAM.
    SaveSettings,
    /// Initialization complete.
    Complete,
    /// Initialization failed with error.
    Failed(ScannerError),
}

// ============================================================================
// Scanner Core
// ============================================================================

/// Sans-IO scanner core.
///
/// This struct manages scanner state and buffer without performing any I/O
/// operations. It provides the core functionality for buffer management,
/// state transitions, and init sequence tracking used by both sync and async
/// scanner implementations.
///
/// # Example
///
/// ```rust,ignore
/// use gm65_scanner::scanner_core::ScannerCore;
/// use gm65_scanner::driver::ScannerConfig;
///
/// let mut core = ScannerCore::with_default_config();
/// core.begin_init();
/// // ... perform I/O operations through driver ...
/// core.complete_init(ScannerModel::Gm65);
/// ```
pub struct ScannerCore {
    state: ScannerState,
    config: ScannerConfig,
    buffer: ScanBuffer,
    initialized: bool,
    detected_model: ScannerModel,
    last_scan_len: Option<usize>,
    /// Current init step for tracking initialization progress.
    init_step: InitStep,
}

impl ScannerCore {
    /// Create a new scanner core with the given configuration.
    pub fn new(config: ScannerConfig) -> Self {
        Self {
            state: ScannerState::Uninitialized,
            config,
            buffer: ScanBuffer::new(),
            initialized: false,
            detected_model: ScannerModel::Unknown,
            last_scan_len: None,
            init_step: InitStep::Start,
        }
    }

    /// Create a new scanner core with default configuration.
    pub fn with_default_config() -> Self {
        Self::new(ScannerConfig::default())
    }

    /// Get the current scanner state.
    pub fn state(&self) -> ScannerState {
        self.state
    }

    /// Check if scanner is initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get the detected scanner model.
    pub fn detected_model(&self) -> ScannerModel {
        self.detected_model
    }

    /// Get the current scanner status.
    pub fn status(&self) -> ScannerStatus {
        ScannerStatus {
            model: self.detected_model,
            connected: self.initialized,
            initialized: self.initialized,
            config: self.config.clone(),
            last_scan_len: self.last_scan_len,
        }
    }

    /// Check if scan data is ready to be read.
    pub fn data_ready(&self) -> bool {
        self.state == ScannerState::ScanComplete
    }

    // ========================================================================
    // Init State Machine
    // ========================================================================

    /// Get the current init step.
    pub fn init_step(&self) -> InitStep {
        self.init_step
    }

    /// Begin initialization sequence.
    ///
    /// Sets state to `Detecting` and init step to `Detecting`.
    pub fn begin_init(&mut self) {
        self.state = ScannerState::Detecting;
        self.init_step = InitStep::Detecting;
    }

    /// Advance to the next init step.
    ///
    /// Updates the internal init step tracker and scanner state as appropriate.
    pub fn advance_init(&mut self, step: InitStep) {
        self.init_step = step;

        match step {
            InitStep::Start => {
                self.state = ScannerState::Uninitialized;
            }
            InitStep::Detecting => {
                self.state = ScannerState::Detecting;
            }
            InitStep::ReadSerialOutput
            | InitStep::FixSerialOutput
            | InitStep::SetCommandMode
            | InitStep::ApplyConfig { .. }
            | InitStep::CheckVersion
            | InitStep::SaveSettings => {
                self.state = ScannerState::Configuring;
            }
            InitStep::Complete => {
                self.state = ScannerState::Ready;
            }
            InitStep::Failed(e) => {
                self.state = ScannerState::Error(e);
            }
        }
    }

    /// Mark scanner as detected (successful probe).
    ///
    /// Sets detected model and transitions to configuring state.
    pub fn mark_detected(&mut self, model: ScannerModel) {
        self.detected_model = model;
        self.state = ScannerState::Configuring;
        self.init_step = InitStep::ReadSerialOutput;
    }

    /// Complete initialization with detected model.
    ///
    /// Sets state to `Ready`, marks initialized, and sets init step to `Complete`.
    pub fn complete_init(&mut self, model: ScannerModel) {
        self.state = ScannerState::Ready;
        self.initialized = true;
        self.detected_model = model;
        self.config.model = model;
        self.init_step = InitStep::Complete;
    }

    /// Fail initialization with an error.
    ///
    /// Sets state to `Error` and init step to `Failed`.
    pub fn fail_init(&mut self, error: ScannerError) {
        self.state = ScannerState::Error(error);
        self.init_step = InitStep::Failed(error);
    }

    // ========================================================================
    // Scan Operations
    // ========================================================================

    /// Begin a scan operation.
    ///
    /// Returns an error if not initialized. Clears the buffer and sets state
    /// to `Scanning`.
    pub fn begin_scan(&mut self) -> Result<(), ScannerError> {
        if !self.initialized {
            return Err(ScannerError::NotInitialized);
        }
        self.state = ScannerState::Scanning;
        self.buffer.clear();
        Ok(())
    }

    /// Process a single scan byte.
    ///
    /// Handles buffer management and detects complete scans (EOL-terminated).
    ///
    /// # Returns
    ///
    /// - `ScanByteResult::NeedMore` - Need more bytes
    /// - `ScanByteResult::Complete(data)` - Complete scan ready
    /// - `ScanByteResult::BufferOverflow` - Buffer overflow detected
    pub fn handle_scan_byte(&mut self, byte: u8) -> ScanByteResult {
        if !self.buffer.push(byte) {
            self.state = ScannerState::Error(ScannerError::BufferOverflow);
            return ScanByteResult::BufferOverflow;
        }

        if self.buffer.has_eol() {
            let data = self.buffer.data_without_eol();
            if data.is_empty() {
                self.buffer.clear();
                return ScanByteResult::NeedMore;
            }

            self.last_scan_len = Some(data.len());
            self.state = ScannerState::ScanComplete;

            // Clone data to avoid holding buffer lock
            let result = data.to_vec();
            self.buffer.clear();
            ScanByteResult::Complete(result)
        } else {
            ScanByteResult::NeedMore
        }
    }

    /// Set an error state.
    ///
    /// Sets state to `Error` with the specified error.
    pub fn fail(&mut self, error: ScannerError) {
        self.state = ScannerState::Error(error);
    }

    /// Get a reference to the scan buffer.
    pub fn buffer(&self) -> &ScanBuffer {
        &self.buffer
    }

    /// Get a mutable reference to the scan buffer.
    pub fn buffer_mut(&mut self) -> &mut ScanBuffer {
        &mut self.buffer
    }

    /// Get the detected model.
    pub fn model(&self) -> ScannerModel {
        self.detected_model
    }

    /// Get the configuration.
    pub fn config(&self) -> &ScannerConfig {
        &self.config
    }

    /// Clear the last scan length.
    pub fn clear_last_scan(&mut self) {
        self.last_scan_len = None;
    }
}

impl Default for ScannerCore {
    fn default() -> Self {
        Self::with_default_config()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::MAX_SCAN_SIZE;
    use crate::driver::ScanMode;

    // ========================================================================
    // ScannerCore Tests
    // ========================================================================

    #[test]
    fn test_new() {
        let config = ScannerConfig {
            model: ScannerModel::Gm65,
            baud_rate: 9600,
            mode: ScanMode::CommandTriggered,
            raw_mode: true,
        };
        let core = ScannerCore::new(config);

        assert_eq!(core.state(), ScannerState::Uninitialized);
        assert!(!core.is_initialized());
        assert_eq!(core.detected_model(), ScannerModel::Unknown);
        assert_eq!(core.init_step(), InitStep::Start);
    }

    #[test]
    fn test_with_default_config() {
        let core = ScannerCore::with_default_config();

        assert_eq!(core.state(), ScannerState::Uninitialized);
        assert!(!core.is_initialized());
        assert_eq!(core.detected_model(), ScannerModel::Unknown);
    }

    #[test]
    fn test_begin_scan_not_initialized() {
        let mut core = ScannerCore::with_default_config();

        let result = core.begin_scan();
        assert_eq!(result, Err(ScannerError::NotInitialized));
    }

    #[test]
    fn test_begin_scan_initialized() {
        let mut core = ScannerCore::with_default_config();
        core.complete_init(ScannerModel::Gm65);

        let result = core.begin_scan();
        assert!(result.is_ok());
        assert_eq!(core.state(), ScannerState::Scanning);
    }

    #[test]
    fn test_handle_scan_byte_need_more() {
        let mut core = ScannerCore::with_default_config();

        // Add bytes without EOL (avoid \r and \n which trigger EOL detection)
        for i in 0..100u8 {
            // Skip \r (13) and \n (10) which would trigger EOL
            if i == b'\r' || i == b'\n' {
                continue;
            }
            let result = core.handle_scan_byte(i);
            assert_eq!(result, ScanByteResult::NeedMore);
        }
    }

    #[test]
    fn test_handle_scan_byte_complete() {
        let mut core = ScannerCore::with_default_config();
        core.complete_init(ScannerModel::Gm65);

        // Simulate a complete scan with EOL
        let scan_data = b"SCANNED_DATA\r\n";
        for &byte in scan_data {
            let result = core.handle_scan_byte(byte);
            match result {
                ScanByteResult::Complete(data) => {
                    assert_eq!(data, b"SCANNED_DATA");
                    assert_eq!(core.last_scan_len, Some(12));
                    return;
                }
                ScanByteResult::NeedMore => continue,
                ScanByteResult::BufferOverflow => panic!("Unexpected overflow"),
            }
        }
        panic!("Scan should have completed");
    }

    #[test]
    fn test_handle_scan_byte_buffer_overflow() {
        let mut core = ScannerCore::with_default_config();

        // Fill buffer to overflow
        for _ in 0..MAX_SCAN_SIZE {
            core.handle_scan_byte(0x00);
        }

        let result = core.handle_scan_byte(0x01);
        assert_eq!(result, ScanByteResult::BufferOverflow);
    }

    #[test]
    fn test_complete_init() {
        let mut core = ScannerCore::with_default_config();
        core.complete_init(ScannerModel::M3Y);

        assert_eq!(core.state(), ScannerState::Ready);
        assert!(core.is_initialized());
        assert_eq!(core.detected_model(), ScannerModel::M3Y);
        assert_eq!(core.init_step(), InitStep::Complete);
    }

    #[test]
    fn test_fail() {
        let mut core = ScannerCore::with_default_config();
        core.fail(ScannerError::NotDetected);

        assert_eq!(core.state(), ScannerState::Error(ScannerError::NotDetected));
    }

    #[test]
    fn test_status() {
        let mut core = ScannerCore::with_default_config();
        core.complete_init(ScannerModel::Gm65);

        let status = core.status();
        assert_eq!(status.model, ScannerModel::Gm65);
        assert!(status.initialized);
        assert!(status.connected);
        assert_eq!(status.last_scan_len, None);
    }

    #[test]
    fn test_data_ready() {
        let mut core = ScannerCore::with_default_config();
        assert!(!core.data_ready());

        core.complete_init(ScannerModel::Gm65);
        let _ = core.begin_scan();
        assert!(!core.data_ready());

        // Simulate scan complete
        core.handle_scan_byte(b't');
        core.handle_scan_byte(b'\r');
        core.handle_scan_byte(b'\n');

        assert!(core.data_ready());
    }

    #[test]
    fn test_buffer_methods() {
        let mut core = ScannerCore::with_default_config();
        let initial_len = core.buffer().len();

        assert_eq!(initial_len, 0);

        core.buffer_mut().push(0x01);
        core.buffer_mut().push(0x02);

        assert_eq!(core.buffer().len(), 2);
    }

    #[test]
    fn test_empty_scan_data() {
        let mut core = ScannerCore::with_default_config();
        core.complete_init(ScannerModel::Gm65);

        // Send just EOL
        let result = core.handle_scan_byte(b'\r');
        assert_eq!(result, ScanByteResult::NeedMore);

        let result = core.handle_scan_byte(b'\n');
        assert_eq!(result, ScanByteResult::NeedMore);
    }

    // ========================================================================
    // Init State Machine Tests
    // ========================================================================

    #[test]
    fn test_begin_init() {
        let mut core = ScannerCore::with_default_config();
        core.begin_init();

        assert_eq!(core.state(), ScannerState::Detecting);
        assert_eq!(core.init_step(), InitStep::Detecting);
    }

    #[test]
    fn test_advance_init() {
        let mut core = ScannerCore::with_default_config();

        core.advance_init(InitStep::Detecting);
        assert_eq!(core.state(), ScannerState::Detecting);
        assert_eq!(core.init_step(), InitStep::Detecting);

        core.advance_init(InitStep::ReadSerialOutput);
        assert_eq!(core.state(), ScannerState::Configuring);
        assert_eq!(core.init_step(), InitStep::ReadSerialOutput);

        core.advance_init(InitStep::ApplyConfig { index: 2 });
        assert_eq!(core.state(), ScannerState::Configuring);
        assert_eq!(core.init_step(), InitStep::ApplyConfig { index: 2 });

        core.advance_init(InitStep::Complete);
        assert_eq!(core.state(), ScannerState::Ready);
        assert_eq!(core.init_step(), InitStep::Complete);
    }

    #[test]
    fn test_advance_init_failure() {
        let mut core = ScannerCore::with_default_config();

        core.advance_init(InitStep::Failed(ScannerError::ConfigFailed));
        assert_eq!(
            core.state(),
            ScannerState::Error(ScannerError::ConfigFailed)
        );
        assert_eq!(
            core.init_step(),
            InitStep::Failed(ScannerError::ConfigFailed)
        );
    }

    #[test]
    fn test_mark_detected() {
        let mut core = ScannerCore::with_default_config();
        core.begin_init();
        core.mark_detected(ScannerModel::Gm65);

        assert_eq!(core.detected_model(), ScannerModel::Gm65);
        assert_eq!(core.state(), ScannerState::Configuring);
        assert_eq!(core.init_step(), InitStep::ReadSerialOutput);
    }

    #[test]
    fn test_fail_init() {
        let mut core = ScannerCore::with_default_config();
        core.begin_init();
        core.fail_init(ScannerError::NotDetected);

        assert_eq!(core.state(), ScannerState::Error(ScannerError::NotDetected));
        assert_eq!(
            core.init_step(),
            InitStep::Failed(ScannerError::NotDetected)
        );
    }

    // ========================================================================
    // Config Constants Tests
    // ========================================================================

    #[test]
    fn test_config_constants() {
        assert_eq!(config::SCAN_INTERVAL_MS, 0x01);
        assert_eq!(config::SAME_BARCODE_DELAY, 0x85);
        assert_eq!(config::CMD_MODE, 0xD1);
        assert_eq!(config::VERSION_NEEDS_RAW, 0x69);
        assert_eq!(config::RAW_MODE_VALUE, 0x08);
    }

    // ========================================================================
    // ScannerSettings Tests
    // ========================================================================

    #[test]
    fn test_scanner_settings_default() {
        let settings = ScannerSettings::default();
        assert!(settings.contains(ScannerSettings::ALWAYS_ON));
        assert!(settings.contains(ScannerSettings::SOUND));
        assert!(settings.contains(ScannerSettings::AIM));
        assert!(settings.contains(ScannerSettings::COMMAND));
        assert!(!settings.contains(ScannerSettings::CONTINUOUS));
    }

    #[test]
    fn test_scanner_settings_bits() {
        // CMD_MODE should be ALWAYS_ON | SOUND | AIM | COMMAND
        let expected = (1 << 7) | (1 << 6) | (1 << 4) | (1 << 0);
        assert_eq!(config::CMD_MODE, expected);

        let settings = ScannerSettings::from_bits(expected);
        assert_eq!(settings, Some(ScannerSettings::default()));
    }

    // ========================================================================
    // Init Config Sequence Tests
    // ========================================================================

    #[test]
    fn test_init_config_sequence() {
        let seq = init_config_sequence();
        assert_eq!(seq.len(), 5);

        // Verify the sequence contains expected registers
        assert_eq!(seq[0].0, Register::Timeout);
        assert_eq!(seq[0].1, 0x00);

        assert_eq!(seq[1].0, Register::ScanInterval);
        assert_eq!(seq[1].1, config::SCAN_INTERVAL_MS);

        assert_eq!(seq[2].0, Register::SameBarcodeDelay);
        assert_eq!(seq[2].1, config::SAME_BARCODE_DELAY);

        assert_eq!(seq[3].0, Register::BarType);
        assert_eq!(seq[3].1, 0x01);

        assert_eq!(seq[4].0, Register::QrEnable);
        assert_eq!(seq[4].1, 0x01);
    }

    #[test]
    fn test_special_registers() {
        let regs = special_registers();
        assert_eq!(regs.len(), 3);

        assert_eq!(regs[0], Register::SerialOutput);
        assert_eq!(regs[1], Register::Settings);
        assert_eq!(regs[2], Register::Version);
    }

    // ========================================================================
    // Serial Output Helpers Tests
    // ========================================================================

    #[test]
    fn test_serial_output_needs_fix() {
        // Bits 0-1 set should need fix
        assert!(serial_output_needs_fix(0x03));
        assert!(serial_output_needs_fix(0xA3));
        assert!(serial_output_needs_fix(0xFF));

        // Bits 0-1 clear should not need fix
        assert!(!serial_output_needs_fix(0x00));
        assert!(!serial_output_needs_fix(0xA0));
        assert!(!serial_output_needs_fix(0xFC));
    }

    #[test]
    fn test_fix_serial_output() {
        // Should clear bits 0-1
        assert_eq!(fix_serial_output(0x03), 0x00);
        assert_eq!(fix_serial_output(0xA3), 0xA0);
        assert_eq!(fix_serial_output(0xFF), 0xFC);

        // Already correct values should remain unchanged
        assert_eq!(fix_serial_output(0x00), 0x00);
        assert_eq!(fix_serial_output(0xA0), 0xA0);
    }

    #[test]
    fn test_serial_output_roundtrip() {
        // If a value needs fixing, the fixed value should not need fixing
        for value in 0..=255u8 {
            if serial_output_needs_fix(value) {
                let fixed = fix_serial_output(value);
                assert!(!serial_output_needs_fix(fixed));
            }
        }
    }

    // ========================================================================
    // Version Helpers Tests
    // ========================================================================

    #[test]
    fn test_version_needs_raw_fix() {
        // Version 0x69 needs fix
        assert!(version_needs_raw_fix(0x69));

        // Other versions don't
        assert!(!version_needs_raw_fix(0x00));
        assert!(!version_needs_raw_fix(0x68));
        assert!(!version_needs_raw_fix(0x6A));
        assert!(!version_needs_raw_fix(0x87));
        assert!(!version_needs_raw_fix(0xFF));
    }

    // ========================================================================
    // InitStep Tests
    // ========================================================================

    #[test]
    fn test_init_step_default() {
        let step = InitStep::default();
        assert_eq!(step, InitStep::Start);
    }

    #[test]
    fn test_init_step_equality() {
        assert_eq!(InitStep::Start, InitStep::Start);
        assert_eq!(
            InitStep::ApplyConfig { index: 2 },
            InitStep::ApplyConfig { index: 2 }
        );
        assert_ne!(
            InitStep::ApplyConfig { index: 1 },
            InitStep::ApplyConfig { index: 2 }
        );
        assert_ne!(InitStep::Detecting, InitStep::Complete);
    }
}
