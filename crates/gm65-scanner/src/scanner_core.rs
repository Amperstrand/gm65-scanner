//! Sans-IO scanner core implementation.
//!
//! Extracts shared logic from sync.rs and async_.rs into a single core
//! that manages state and buffer without I/O operations.

extern crate alloc;

use crate::buffer::ScanBuffer;
use crate::driver::{ScannerConfig, ScannerError, ScannerModel, ScannerState, ScannerStatus};

/// Result of processing a scan byte
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ScanByteResult {
    /// Need more bytes to complete scan
    NeedMore,
    /// Complete scan data ready
    Complete(alloc::vec::Vec<u8>),
    /// Buffer overflow detected
    BufferOverflow,
}

/// Initialization step tracker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InitStep {
    #[default]
    Start,
    Detecting,
    ReadSerialOutput,
    FixSerialOutput,
    SetCommandMode,
    ApplyConfig {
        index: usize,
    },
    CheckVersion,
    SaveSettings,
    Complete,
    Failed(ScannerError),
}

/// Sans-IO scanner core.
///
/// This struct manages scanner state and buffer without performing any I/O
/// operations. It provides the core functionality for buffer management and
/// state transitions used by both sync and async scanner implementations.
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

    /// Begin initialization sequence.
    ///
    /// Sets state to `Detecting` and prepares for model detection.
    pub fn begin_init(&mut self) {
        self.state = ScannerState::Detecting;
    }

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

    /// Complete initialization with detected model.
    ///
    /// Sets state to `Ready` and marks initialized.
    pub fn complete_init(&mut self, model: ScannerModel) {
        self.state = ScannerState::Ready;
        self.initialized = true;
        self.detected_model = model;
        self.config.model = model;
    }

    /// Set an error state.
    ///
    /// Sets state to `Error` with the specified error.
    pub fn fail(&mut self, error: ScannerError) {
        self.state = ScannerState::Error(error);
    }

    /// Get the current initialization step.
    pub fn init_step(&self) -> InitStep {
        match self.state {
            ScannerState::Uninitialized => InitStep::Start,
            ScannerState::Detecting => InitStep::Detecting,
            ScannerState::Configuring => {
                // Try to infer step from internal state if needed
                // This is a simplified version
                InitStep::Complete
            }
            ScannerState::Ready => InitStep::Complete,
            ScannerState::Scanning => InitStep::ApplyConfig {
                index: self.last_scan_len.unwrap_or(0),
            },
            ScannerState::ScanComplete => InitStep::Complete,
            ScannerState::Error(e) => InitStep::Failed(e),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::MAX_SCAN_SIZE;
    use crate::driver::ScanMode;

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
        core.begin_scan();
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
    fn test_init_step() {
        let mut core = ScannerCore::with_default_config();
        assert_eq!(core.init_step(), InitStep::Start);

        core.begin_init();
        assert_eq!(core.init_step(), InitStep::Detecting);

        core.complete_init(ScannerModel::Gm65);
        assert_eq!(core.init_step(), InitStep::Complete);

        core.fail(ScannerError::NotDetected);
        assert_eq!(
            core.init_step(),
            InitStep::Failed(ScannerError::NotDetected)
        );
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
}
