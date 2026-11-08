//! Multi-baud init support (#85, built for real this time): the host-side
//! baud-switch hook plus the driver sequence that uses it.
//!
//! The driver is transport-agnostic — it cannot reconfigure the host UART.
//! Consumers implement [`BaudSwitch`] (one method, backed by their UART's
//! runtime baud control — on their own wrapper type, orphan rules forbid
//! implementing it on foreign UART types) and call
//! [`crate::Gm65ScannerAsync::init_multi_baud`]:
//!
//! 1. init at the current baud (normal case — module already at 115200)
//! 2. on `NotDetected`: walk the recovery ladder — factory-default 9600,
//!    then 57600 (the M3Y retain rate, `docs/GM65-PROTOCOL-FINDINGS.md`
//!    "Multi-Baud Rate Probing"; #107) — initing at each step
//! 3. if a probe answered: command the module to 115200, switch the host up,
//!    and finish with a full init at operating baud

use crate::driver::types::ScannerError;

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

/// Ladder failure that still owns the scanner. The UART is a singleton
/// peripheral (plus DMA tokens) — dropping it on error would make a
/// retry impossible, so the failed ladder hands the live scanner back
/// with the error instead (#108). The host UART is left at operating
/// baud (115200).
pub struct InitMultiBaudError<S> {
    pub error: ScannerError,
    pub scanner: S,
}

impl<S> InitMultiBaudError<S> {
    pub fn into_parts(self) -> (ScannerError, S) {
        (self.error, self.scanner)
    }
}

impl<S> core::fmt::Debug for InitMultiBaudError<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InitMultiBaudError")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// The common-case ladder: build a scanner on a UART that controls its own
/// baud (implement [`BaudSwitch`] on your own wrapper type), probe 115200,
/// fall to the factory-default 9600, and land back at 115200 — returning a
/// ready scanner. Owns the take-apart/put-back dance internally so
/// consumers never duplicate it (B28: module logic lives in the crate).
/// On failure the scanner comes back inside [`InitMultiBaudError`] — the
/// caller keeps a live handle to retry (#108).
///
/// ```rust
/// use gm65_scanner::{init_scanner_multi_baud, BaudSwitch, ScannerError};
///
/// // Implementing a foreign trait for a foreign type — e.g.
/// // `impl BaudSwitch for embassy_stm32::usart::BufferedUart<'_>` —
/// // violates the orphan rule and will not compile downstream. Wrap the
/// // UART in your own type and implement the hook (plus the
/// // `embedded_io_async` traits) on the wrapper:
/// struct SwitchableUart<U>(U);
///
/// impl<U> BaudSwitch for SwitchableUart<U> {
///     fn set_baud(&mut self, baud: u32) -> bool {
///         // e.g. self.0.set_baudrate(baud).is_ok()
///         let _ = baud;
///         true
///     }
/// }
/// #
/// # use embedded_io_async::{ErrorType, Read, Write};
/// # struct DeadUart;
/// # impl ErrorType for DeadUart { type Error = embedded_io_async::ErrorKind; }
/// # impl Write for DeadUart {
/// #     async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> { Ok(buf.len()) }
/// #     async fn flush(&mut self) -> Result<(), Self::Error> { Ok(()) }
/// # }
/// # impl Read for DeadUart {
/// #     async fn read(&mut self, _buf: &mut [u8]) -> Result<usize, Self::Error> {
/// #         Err(embedded_io_async::ErrorKind::Other)
/// #     }
/// # }
/// # // The wrapper forwards the IO traits to the inner UART — a real
/// # // consumer's wrapper does the same delegation.
/// # impl<U: ErrorType> ErrorType for SwitchableUart<U> { type Error = U::Error; }
/// # impl<U: Read> Read for SwitchableUart<U> {
/// #     async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> { self.0.read(buf).await }
/// # }
/// # impl<U: Write> Write for SwitchableUart<U> {
/// #     async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> { self.0.write(buf).await }
/// #     async fn flush(&mut self) -> Result<(), Self::Error> { self.0.flush().await }
/// # }
/// # // Doctests are separate crates: the lib's cfg(test) time-driver
/// # // shim (async_.rs) is not visible, so the rig carries its own.
/// # use std::sync::atomic::{AtomicU64, Ordering};
/// # static MOCK_TIME: AtomicU64 = AtomicU64::new(1_000_000_000_000u64);
/// # #[no_mangle]
/// # extern "C" fn _embassy_time_now() -> u64 { MOCK_TIME.load(Ordering::Relaxed) }
/// # #[no_mangle]
/// # extern "C" fn _embassy_time_schedule_wake(at: u64) { MOCK_TIME.store(at, Ordering::Relaxed) }
/// #
/// # let outcome = futures_executor::block_on(async {
/// #     init_scanner_multi_baud(SwitchableUart(DeadUart)).await
/// # });
/// # let err = match outcome { Ok(_) => panic!("dead uart cannot init"), Err(e) => e };
/// # assert_eq!(err.error, ScannerError::NotDetected);
/// # let (_uart, _state, _data, _model) = err.scanner.into_parts();
/// ```
///
/// The error owns the (still-live) scanner — `into_parts()` hands the
/// UART back so the caller can retry after, say, power-cycling the
/// module (#108).
// The Err variant is large by design: it carries the hardware handle
// back to the caller (#108). It is moved, never copied.
#[allow(clippy::result_large_err)]
#[cfg(feature = "async")]
pub async fn init_scanner_multi_baud<UART>(
    uart: UART,
) -> Result<
    (crate::Gm65ScannerAsync<UART>, crate::ScannerModel),
    InitMultiBaudError<crate::Gm65ScannerAsync<UART>>,
>
where
    UART: embedded_io_async::Write + embedded_io_async::Read + crate::driver::BaudSwitch,
{
    use crate::driver::ScannerDriver as _;

    let mut scanner = crate::Gm65ScannerAsync::with_default_config(uart);
    match scanner.init().await {
        Ok(model) => return Ok((scanner, model)),
        Err(ScannerError::NotDetected) => {}
        Err(e) => return Err(InitMultiBaudError { error: e, scanner }),
    }

    // Recovery ladder: factory-default 9600, then 57600 (M3Y retain
    // rate — findings doc "Multi-Baud Rate Probing"; #107).
    for probe in [9600u32, 57600] {
        let (mut uart, ..) = scanner.into_parts();
        if !uart.set_baud(probe) {
            // Host can't leave the current rate — restore operating baud.
            let _ = uart.set_baud(115200);
            let scanner = crate::Gm65ScannerAsync::with_default_config(uart);
            return Err(InitMultiBaudError {
                error: ScannerError::NotDetected,
                scanner,
            });
        }
        scanner = crate::Gm65ScannerAsync::with_default_config(uart);
        match scanner.init().await {
            Ok(_) => {
                // Module answered at `probe`: move it (and the UART) to
                // operating baud and verify with a full init.
                let _ = scanner.set_baud_115200().await;
                let (mut uart, ..) = scanner.into_parts();
                if !uart.set_baud(115200) {
                    let scanner = crate::Gm65ScannerAsync::with_default_config(uart);
                    return Err(InitMultiBaudError {
                        error: ScannerError::UartError,
                        scanner,
                    });
                }
                let mut scanner = crate::Gm65ScannerAsync::with_default_config(uart);
                match scanner.init().await {
                    Ok(model) => return Ok((scanner, model)),
                    Err(e) => {
                        return Err(InitMultiBaudError { error: e, scanner });
                    }
                }
            }
            Err(ScannerError::NotDetected) => {}
            Err(e) => {
                // Real fault at the probe rate — restore the host to
                // operating baud before handing the live scanner back,
                // or a retry succeeds at the stale probe rate and skips
                // normalization entirely (Codex P2 on #115; the method
                // twins already restored here).
                let (mut uart, ..) = scanner.into_parts();
                let _ = uart.set_baud(115200);
                let scanner = crate::Gm65ScannerAsync::with_default_config(uart);
                return Err(InitMultiBaudError { error: e, scanner });
            }
        }
    }

    // Nothing answered anywhere — leave the UART at operating baud and
    // hand the live scanner back so the caller can retry (#108).
    let (mut uart, ..) = scanner.into_parts();
    let _ = uart.set_baud(115200);
    let scanner = crate::Gm65ScannerAsync::with_default_config(uart);
    Err(InitMultiBaudError {
        error: ScannerError::NotDetected,
        scanner,
    })
}

/// Sync mirror of [`init_scanner_multi_baud`]: the same ladder for the
/// blocking driver, with the same error-owns-the-scanner contract (#108).
// The Err variant is large by design: it carries the hardware handle
// back to the caller (#108). It is moved, never copied.
#[allow(clippy::result_large_err)]
#[cfg(feature = "sync")]
pub fn init_scanner_multi_baud_sync<UART>(
    uart: UART,
) -> Result<
    (crate::Gm65Scanner<UART>, crate::ScannerModel),
    InitMultiBaudError<crate::Gm65Scanner<UART>>,
>
where
    UART: embedded_hal_02::serial::Write<u8>
        + embedded_hal_02::serial::Read<u8>
        + crate::driver::BaudSwitch,
{
    use crate::driver::ScannerDriverSync as _;

    let mut scanner = crate::Gm65Scanner::with_default_config(uart);
    match scanner.init() {
        Ok(model) => return Ok((scanner, model)),
        Err(ScannerError::NotDetected) => {}
        Err(e) => return Err(InitMultiBaudError { error: e, scanner }),
    }

    for probe in [9600u32, 57600] {
        let (mut uart, ..) = scanner.into_parts();
        if !uart.set_baud(probe) {
            let _ = uart.set_baud(115200);
            let scanner = crate::Gm65Scanner::with_default_config(uart);
            return Err(InitMultiBaudError {
                error: ScannerError::NotDetected,
                scanner,
            });
        }
        scanner = crate::Gm65Scanner::with_default_config(uart);
        match scanner.init() {
            Ok(_) => {
                let _ = scanner.set_baud_115200();
                let (mut uart, ..) = scanner.into_parts();
                if !uart.set_baud(115200) {
                    let scanner = crate::Gm65Scanner::with_default_config(uart);
                    return Err(InitMultiBaudError {
                        error: ScannerError::UartError,
                        scanner,
                    });
                }
                let mut scanner = crate::Gm65Scanner::with_default_config(uart);
                match scanner.init() {
                    Ok(model) => return Ok((scanner, model)),
                    Err(e) => {
                        return Err(InitMultiBaudError { error: e, scanner });
                    }
                }
            }
            Err(ScannerError::NotDetected) => {}
            Err(e) => {
                // Same restore as the async twin (Codex P2 on #115).
                let (mut uart, ..) = scanner.into_parts();
                let _ = uart.set_baud(115200);
                let scanner = crate::Gm65Scanner::with_default_config(uart);
                return Err(InitMultiBaudError { error: e, scanner });
            }
        }
    }

    let (mut uart, ..) = scanner.into_parts();
    let _ = uart.set_baud(115200);
    let scanner = crate::Gm65Scanner::with_default_config(uart);
    Err(InitMultiBaudError {
        error: ScannerError::NotDetected,
        scanner,
    })
}
