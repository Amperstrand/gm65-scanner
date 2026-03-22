//! Async Scanner Firmware — embassy executor
//!
//! Concurrent tasks: scanner, USB CDC, LED indicator.
//! Scanner results are echoed over USB CDC with type prefix.
//! LED (PG6) blinks on successful scan.
//!
//! Run: cargo run --release --target thumbv7em-none-eabihf --bin async_firmware --features scanner-async,defmt

#![no_std]
#![no_main]

extern crate alloc;

#[cfg(feature = "scanner-async")]
use alloc::string::String;
#[cfg(feature = "scanner-async")]
use alloc::vec::Vec;

#[cfg(feature = "scanner-async")]
use defmt_rtt as _;
#[cfg(feature = "scanner-async")]
use panic_probe as _;

#[cfg(feature = "scanner-async")]
use embassy_executor::Spawner;
#[cfg(feature = "scanner-async")]
use embassy_stm32::{bind_interrupts, interrupt::InterruptExt, peripherals, rcc::*, usart, usb, Config};
#[cfg(feature = "scanner-async")]
use embassy_stm32::time::Hertz;
#[cfg(feature = "scanner-async")]
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
#[cfg(feature = "scanner-async")]
use embassy_sync::channel::Channel;
#[cfg(feature = "scanner-async")]
use embassy_time::{Duration, Ticker, Timer};
#[cfg(feature = "scanner-async")]
use embassy_usb::Builder;
#[cfg(feature = "scanner-async")]
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
#[cfg(feature = "scanner-async")]
use embedded_hal_02::blocking::serial::Write as _;
#[cfg(feature = "scanner-async")]
use embedded_io::ErrorType;
#[cfg(feature = "scanner-async")]
use gm65_scanner::{Gm65ScannerAsync, ScannerDriver};
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
static SCAN_CHANNEL: Channel<CriticalSectionRawMutex, ScanResult, 4> = Channel::new();

#[cfg(feature = "scanner-async")]
#[derive(Clone)]
pub struct ScanResult {
    pub data: Vec<u8>,
}

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
bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

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

    let mut config = Config::default();
    {
        config.rcc.hse = Some(Hse {
            freq: Hertz(8_000_000),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL168,
            divp: Some(PllPDiv::DIV2),
            divq: Some(PllQDiv::DIV7),
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.mux.clk48sel = mux::Clk48sel::PLL1_Q;
    }
    let p = embassy_stm32::init(config);

    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;
    let uart = usart::Uart::new_blocking(p.USART6, p.PG9, p.PG14, uart_config).unwrap();
    embassy_stm32::interrupt::USART6.disable();

    let async_uart = AsyncUart { inner: uart };
    let mut scanner = Gm65ScannerAsync::with_default_config(async_uart);

    let mut led = embassy_stm32::gpio::Output::new(p.PG6, embassy_stm32::gpio::Level::Low, embassy_stm32::gpio::Speed::Low);

    let mut ep_out_buffer = [0u8; 256];
    let mut usb_config = usb::Config::default();
    usb_config.vbus_detection = false;
    let usb_driver = usb::Driver::new_fs(p.USB_OTG_FS, Irqs, p.PA12, p.PA11, &mut ep_out_buffer, usb_config);

    let mut usb_config_desc = embassy_usb::Config::new(0xc0de, 0xcafe);
    usb_config_desc.manufacturer = Some("gm65-scanner");
    usb_config_desc.product = Some("QR Scanner");
    usb_config_desc.serial_number = Some("f469disco");

    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut msos_descriptor = [0; 256];
    let mut control_buf = [0; 64];

    let mut usb_state = State::new();
    let mut usb_builder = Builder::new(
        usb_driver,
        usb_config_desc,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );
    let mut cdc = CdcAcmClass::new(&mut usb_builder, &mut usb_state, 64);
    let mut usb_dev = usb_builder.build();

    defmt::info!("Async scanner firmware started (168MHz, USB CDC ready)");

    let scanner_task = async {
        defmt::info!("Scanner: initializing...");
        match scanner.init().await {
            Ok(model) => defmt::info!("Scanner: detected {:?}", model),
            Err(e) => {
                defmt::error!("Scanner: init failed {:?}", e);
                loop {
                    Timer::after(Duration::from_secs(1)).await;
                }
            }
        }

        loop {
            defmt::info!("Scanner: waiting for QR code...");
            if scanner.trigger_scan().await.is_err() {
                defmt::error!("Scanner: trigger failed");
                Timer::after(Duration::from_millis(500)).await;
                continue;
            }

            match embassy_time::with_timeout(Duration::from_secs(10), scanner.read_scan()).await {
                Ok(Some(data)) => {
                    let len = data.len();
                    defmt::info!("Scanner: scanned {} bytes", len);
                    let _ = SCAN_CHANNEL.try_send(ScanResult { data });
                    for _ in 0..3 {
                        led.set_high();
                        Timer::after(Duration::from_millis(100)).await;
                        led.set_low();
                        Timer::after(Duration::from_millis(100)).await;
                    }
                }
                Ok(None) => {
                    defmt::info!("Scanner: timeout (no QR code)");
                    scanner.cancel_scan();
                    let _ = scanner.stop_scan().await;
                }
                Err(_) => {
                    defmt::info!("Scanner: timeout expired");
                    scanner.cancel_scan();
                    let _ = scanner.stop_scan().await;
                }
            }
        }
    };

    let usb_task = async {
        usb_dev.run().await;
    };

    let cdc_task = async {
        loop {
            cdc.wait_connection().await;
            defmt::info!("USB: connected");

            let mut heartbeat = Ticker::every(Duration::from_secs(3));
            loop {
                match SCAN_CHANNEL.try_receive() {
                    Ok(result) => {
                        let data_str = String::from_utf8_lossy(&result.data);
                        let header = "[SCAN] ";
                        let mut msg = String::from(header);
                        msg.push_str(&data_str);
                        msg.push_str("\r\n");
                        match cdc.write_packet(msg.as_bytes()).await {
                            Ok(()) => defmt::info!("USB: sent scan result"),
                            Err(_) => break,
                        }
                    }
                    Err(_) => {
                        heartbeat.next().await;
                        match cdc.write_packet(b"[ALIVE] gm65-scanner ready\r\n").await {
                            Ok(()) => {}
                            Err(_) => break,
                        }
                    }
                }
            }
            defmt::info!("USB: disconnected");
            Timer::after(Duration::from_millis(100)).await;
        }
    };

    embassy_futures::join::join3(usb_task, scanner_task, cdc_task).await;
}

#[cfg(not(feature = "scanner-async"))]
use defmt_rtt as _;

#[cfg(not(feature = "scanner-async"))]
#[cortex_m_rt::entry]
fn main() -> ! {
    defmt::error!("This binary requires the 'scanner-async' feature");
    loop {
        cortex_m::asm::wfi();
    }
}
