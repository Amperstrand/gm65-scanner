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

/// The common-case ladder: build a scanner on a UART that controls its own
/// baud (implement [`BaudSwitch`] on your UART type), probe 115200, fall
/// to the factory-default 9600, and land back at 115200 — returning a
/// ready scanner. Owns the take-apart/put-back dance internally so
/// consumers never duplicate it (B28: module logic lives in the crate).
///
/// ```ignore
/// impl gm65_scanner::BaudSwitch for embassy_stm32::usart::BufferedUart<'_> {
///     fn set_baud(&mut self, baud: u32) -> bool { self.set_baudrate(baud).is_ok() }
/// }
/// let (mut scanner, model) = gm65_scanner::init_scanner_multi_baud(uart).await?;
/// scanner.start_scanning(ScanPolicy::SilentContinuous).await?;
/// ```
pub async fn init_scanner_multi_baud<UART>(
    uart: UART,
) -> Result<(crate::Gm65ScannerAsync<UART>, crate::ScannerModel), crate::ScannerError>
where
    UART: embedded_io_async::Write + embedded_io_async::Read + crate::driver::BaudSwitch,
{
    use crate::driver::ScannerDriver as _;

    let mut scanner = crate::Gm65ScannerAsync::with_default_config(uart);
    match scanner.init().await {
        Ok(model) => return Ok((scanner, model)),
        Err(crate::ScannerError::NotDetected) => {}
        Err(e) => return Err(e),
    }

    // Factory-default baud probe.
    let (mut uart, ..) = scanner.into_parts();
    if !uart.set_baud(9600) {
        return Err(crate::ScannerError::UartError);
    }
    let mut scanner = crate::Gm65ScannerAsync::with_default_config(uart);
    match scanner.init().await {
        Ok(_) => {
            // Module answered at 9600: move it (and the UART) to operating
            // baud and verify with a full init.
            let _ = scanner.set_baud_115200().await;
            let (mut uart, ..) = scanner.into_parts();
            if !uart.set_baud(115200) {
                return Err(crate::ScannerError::UartError);
            }
            let mut scanner = crate::Gm65ScannerAsync::with_default_config(uart);
            let model = scanner.init().await?;
            Ok((scanner, model))
        }
        Err(e) => {
            // Nothing at 9600 either — leave the UART at operating baud
            // and rebuild the scanner so the caller keeps a live handle.
            let (mut uart, ..) = scanner.into_parts();
            let _ = uart.set_baud(115200);
            let scanner = crate::Gm65ScannerAsync::with_default_config(uart);
            let _ = scanner; // caller receives Err; drop quietly
            Err(e)
        }
    }
}
