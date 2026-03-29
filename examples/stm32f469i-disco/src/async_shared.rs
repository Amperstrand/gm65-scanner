#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn LTDC() {
    cortex_m::asm::nop();
}
#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn LTDC_ER() {
    cortex_m::asm::nop();
}
#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DSI() {
    cortex_m::asm::nop();
}
#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DSIHOST() {
    cortex_m::asm::nop();
}
#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DMA2D() {
    cortex_m::asm::nop();
}
#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn FMC() {
    cortex_m::asm::nop();
}

#[cfg(feature = "scanner-async")]
use embedded_hal_02::blocking::serial::Write as _;

#[cfg(feature = "scanner-async")]
pub struct AsyncUart<'d> {
    pub inner: embassy_stm32::usart::Uart<'d, embassy_stm32::mode::Blocking>,
    pub yield_threshold: u32,
}

#[cfg(feature = "scanner-async")]
impl<'d> embedded_io::ErrorType for AsyncUart<'d> {
    type Error = embassy_stm32::usart::Error;
}

#[cfg(feature = "scanner-async")]
impl<'d> embedded_io_async::Read for AsyncUart<'d> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut total = 0usize;
        for slot in buf.iter_mut() {
            let mut spins = 0u32;
            loop {
                match embedded_hal_02::serial::Read::read(&mut self.inner) {
                    Ok(byte) => {
                        *slot = byte;
                        total += 1;
                        break;
                    }
                    Err(nb::Error::WouldBlock) => {
                        spins += 1;
                        if spins < self.yield_threshold {
                            continue;
                        }
                        embassy_time::Timer::after_micros(100).await;
                    }
                    Err(nb::Error::Other(_e)) => {
                        unsafe {
                            const USART6_BASE: usize = 0x4001_1400;
                            let _sr = core::ptr::read_volatile(USART6_BASE as *const u32);
                            let _dr = core::ptr::read_volatile((USART6_BASE + 0x04) as *const u32);
                        }
                        embassy_time::Timer::after_micros(10).await;
                    }
                }
            }
        }
        Ok(total)
    }
}

#[cfg(feature = "scanner-async")]
impl<'d> embedded_io_async::Write for AsyncUart<'d> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.inner.bwrite_all(buf)?;
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.bflush()
    }
}
