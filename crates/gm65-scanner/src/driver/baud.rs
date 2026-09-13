//! Multi-baud init support (#85, built for real this time): the host-side
//! baud-switch hook plus the driver sequence that uses it.
//!
//! The driver is transport-agnostic — it cannot reconfigure the host UART.
//! Consumers implement [`BaudSwitch`] (one method, backed by their UART's
//! runtime baud control) and call [`crate::Gm65ScannerAsync::init_multi_baud`]:
//!
//! 1. init at the current baud (normal case — module already at 115200)
//! 2. on `NotDetected`: switch the host to the factory-default 9600 and
//!    init again (recovers modules whose NVRAM baud drifted or that were
//!    factory-reset by a heal)
//! 3. if 9600 answered: command the module to 115200, switch the host up,
//!    and finish with a full init at operating baud

/// Host hook: switch the underlying UART's baud rate.
pub trait BaudSwitch {
    /// Reconfigure the host UART. Returns success; `false` aborts the
    /// multi-baud ladder (the driver stays on the current baud).
    fn set_baud(&mut self, baud: u32) -> bool;
}

/// No-op switch for hosts with a fixed UART — `init_multi_baud` degrades
/// to a plain init retry.
pub struct NoBaudSwitch;
impl BaudSwitch for NoBaudSwitch {
    fn set_baud(&mut self, _baud: u32) -> bool {
        false
    }
}
