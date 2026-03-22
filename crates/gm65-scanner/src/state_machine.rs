//! GM65 Scanner State Machine (Re-exports)
//!
//! This module re-exports types and functions from `scanner_core` for backwards
//! compatibility. All functionality has been consolidated into `scanner_core`.

pub use crate::scanner_core::{
    config, fix_serial_output, init_config_sequence, serial_output_needs_fix, special_registers,
    version_needs_raw_fix, RegisterConfig, ScannerSettings,
};
