//! Unified I/O abstraction for scanner drivers.
//!
//! Defines the byte-level I/O contract used by the protocol logic.
//! Both sync (nb-based) and async (embedded-io-async) UART types
//! implement these traits, enabling future unification via maybe-async.

#[cfg(feature = "sync")]
pub trait SyncScannerIO {
    fn write_all(&mut self, data: &[u8]) -> Result<(), ()>;
    fn read_byte(&mut self) -> Option<u8>;
    fn flush(&mut self) -> Result<(), ()>;
}

#[cfg(feature = "async")]
pub trait AsyncScannerIO {
    async fn write_all(&mut self, data: &[u8]) -> Result<(), ()>;
    async fn read_byte(&mut self) -> Option<u8>;
    async fn flush(&mut self) -> Result<(), ()>;
}

#[cfg(feature = "sync")]
impl<UART> SyncScannerIO for UART
where
    UART: embedded_hal_02::serial::Write<u8> + embedded_hal_02::serial::Read<u8>,
{
    fn write_all(&mut self, data: &[u8]) -> Result<(), ()> {
        for &byte in data {
            let mut attempts = 0u32;
            loop {
                match embedded_hal_02::serial::Write::write(self, byte) {
                    Ok(()) => break,
                    Err(nb::Error::WouldBlock) => {
                        attempts += 1;
                        if attempts > 100_000 {
                            return Err(());
                        }
                    }
                    Err(_) => return Err(()),
                }
            }
        }
        Ok(())
    }

    fn read_byte(&mut self) -> Option<u8> {
        match embedded_hal_02::serial::Read::read(self) {
            Ok(b) => Some(b),
            Err(nb::Error::WouldBlock) => None,
            Err(_) => None,
        }
    }

    fn flush(&mut self) -> Result<(), ()> {
        loop {
            match embedded_hal_02::serial::Write::flush(self) {
                Ok(()) => return Ok(()),
                Err(nb::Error::WouldBlock) => continue,
                Err(_) => return Err(()),
            }
        }
    }
}

#[cfg(feature = "async")]
impl<UART> AsyncScannerIO for UART
where
    UART: embedded_io_async::Read + embedded_io_async::Write,
{
    async fn write_all(&mut self, data: &[u8]) -> Result<(), ()> {
        use embedded_io_async::Write;
        Write::write_all(self, data).await.map_err(|_| ())
    }

    async fn read_byte(&mut self) -> Option<u8> {
        use embedded_io_async::Read;
        let mut buf = [0u8; 1];
        match Read::read(self, &mut buf).await {
            Ok(1) => Some(buf[0]),
            _ => None,
        }
    }

    async fn flush(&mut self) -> Result<(), ()> {
        use embedded_io_async::Write;
        Write::flush(self).await.map_err(|_| ())
    }
}
