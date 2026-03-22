//! Async HIL Test Binary for Async Driver
//!
//! Uses embassy executor + AsyncUart wrapper around embassy-stm32 blocking UART.
//!
//! Run: cargo run --release --target thumbv7em-none-eabihf --bin hil_test_async --features scanner-async,defmt

#![no_std]
#![no_main]

extern crate alloc;

#[cfg(feature = "scanner-async")]
use defmt_rtt as _;
#[cfg(feature = "scanner-async")]
use panic_probe as _;

#[cfg(feature = "scanner-async")]
use embassy_executor::Spawner;
#[cfg(feature = "scanner-async")]
use embassy_stm32::{interrupt::InterruptExt, usart, Config};
#[cfg(feature = "scanner-async")]
use embassy_time::Timer;
#[cfg(feature = "scanner-async")]
use embedded_hal_02::blocking::serial::Write as _;
#[cfg(feature = "scanner-async")]
use embedded_io::ErrorType;
#[cfg(feature = "scanner-async")]
use gm65_scanner::{Gm65ScannerAsync, driver::async_hil_tests as hil_tests};
#[cfg(feature = "scanner-async")]
use linked_list_allocator::LockedHeap;

#[cfg(feature = "scanner-async")]
const HEAP_SIZE: usize = 32 * 1024;
#[cfg(feature = "scanner-async")]
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();
#[cfg(feature = "scanner-async")]
static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn LTDC() { cortex_m::asm::nop(); }
#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn LTDC_ER() { cortex_m::asm::nop(); }
#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DSI() { cortex_m::asm::nop(); }
#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DSIHOST() { cortex_m::asm::nop(); }
#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DMA2D() { cortex_m::asm::nop(); }
#[cfg(feature = "scanner-async")]
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn FMC() { cortex_m::asm::nop(); }

#[cfg(feature = "scanner-async")]
struct AsyncUart<'d> {
    inner: usart::Uart<'d, embassy_stm32::mode::Blocking>,
}

#[cfg(feature = "scanner-async")]
impl<'d> ErrorType for AsyncUart<'d> {
    type Error = usart::Error;
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
                        if spins < 100_000 {
                            continue;
                        }
                        Timer::after_micros(100).await;
                    }
                    Err(nb::Error::Other(_e)) => {
                        unsafe {
                            const USART6_BASE: usize = 0x4001_1400;
                            let _sr = core::ptr::read_volatile(USART6_BASE as *const u32);
                            let _dr = core::ptr::read_volatile((USART6_BASE + 0x04) as *const u32);
                        }
                        Timer::after_micros(10).await;
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

#[cfg(feature = "scanner-async")]
#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    unsafe {
        ALLOCATOR
            .lock()
            .init(core::ptr::addr_of_mut!(HEAP_MEMORY) as *mut u8, HEAP_SIZE);
    }

    defmt::info!("========================================");
    defmt::info!("Async HIL Tests (embassy executor)");
    defmt::info!("========================================");

    let p = embassy_stm32::init(Config::default());

    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;

    let uart = usart::Uart::new_blocking(p.USART6, p.PG9, p.PG14, uart_config).unwrap();
    embassy_stm32::interrupt::USART6.disable();

    let async_uart = AsyncUart { inner: uart };
    let mut scanner = Gm65ScannerAsync::with_default_config(async_uart);

    let results = hil_tests::run_hil_tests(&mut scanner).await;

    if results.all_passed() {
        defmt::info!("All async HIL tests passed!");
    } else {
        defmt::error!("Async HIL tests: {}/5 passed", results.passed_count());
    }

    if results.all_passed() {
        defmt::info!("========================================");
        defmt::info!("QR Scan Test - present QR code now!");
        defmt::info!("========================================");
        let qr_result = hil_tests::run_hil_test_with_qr(&mut scanner).await;
        if qr_result {
            defmt::info!("QR SCAN TEST PASSED!");
        } else {
            defmt::error!("QR SCAN TEST FAILED");
        }
    }

    defmt::info!("Done. Looping forever.");
    loop {}
}

#[cfg(not(feature = "scanner-async"))]
use defmt_rtt as _;

#[cfg(not(feature = "scanner-async"))]
#[entry]
fn main() -> ! {
    defmt::error!("This binary requires the 'scanner-async' feature");
    loop {
        cortex_m::asm::wfi();
    }
}
