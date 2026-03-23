//! Async Scanner Firmware — embassy executor
//!
//! Concurrent tasks: scanner, USB CDC, LED indicator, SDRAM.
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
use embassy_stm32::time::Hertz;
#[cfg(feature = "scanner-async")]
use embassy_stm32::{
    bind_interrupts, interrupt::InterruptExt, peripherals, rcc::*, usart, usb, Config,
};
#[cfg(feature = "scanner-async")]
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
#[cfg(feature = "scanner-async")]
use embassy_sync::channel::Channel;
#[cfg(feature = "scanner-async")]
use embassy_time::{Duration, Ticker, Timer};
#[cfg(feature = "scanner-async")]
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
#[cfg(feature = "scanner-async")]
use embassy_usb::Builder;
#[cfg(feature = "scanner-async")]
use embedded_hal_02::blocking::serial::Write as _;
#[cfg(feature = "scanner-async")]
use embedded_io::ErrorType;
#[cfg(feature = "scanner-async")]
use gm65_scanner::{Gm65ScannerAsync, ScannerDriver};
#[cfg(feature = "scanner-async")]
use linked_list_allocator::LockedHeap;

#[cfg(feature = "scanner-async")]
use crate::display_embassy::SdramCtrl;

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
static SDRAM_CHANNEL: Channel<CriticalSectionRawMutex, SdramStatus, 4> = Channel::new();

#[cfg(feature = "scanner-async")]
static DISPLAY_CHANNEL: Channel<CriticalSectionRawMutex, ScanResult, 4> = Channel::new();

#[cfg(feature = "scanner-async")]
#[derive(Clone)]
pub struct ScanResult {
    pub data: Vec<u8>,
}

#[cfg(feature = "scanner-async")]
#[derive(Clone)]
pub struct SdramStatus {
    pub base_address: usize,
    pub test_passed: bool,
}

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
        let yield_threshold = if buf.len() <= 8 { 2_000_000 } else { 100_000 };
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
                        if spins < yield_threshold {
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
        config.rcc.pllsai = Some(Pll {
            prediv: PllPreDiv::DIV8,
            mul: PllMul::MUL384,
            divp: None,
            divq: None,
            divr: Some(PllRDiv::DIV7),
        });
    }
    let mut p = embassy_stm32::init(config);

    defmt::info!("Initializing SDRAM...");
    let sdram = SdramCtrl::new(&mut p, 168_000_000);
    let sdram_base = sdram.base_address();
    let sdram_ok = sdram.test_quick();
    defmt::info!("SDRAM: base={:#010x} test={}", sdram_base, sdram_ok);
    let _ = SDRAM_CHANNEL.try_send(SdramStatus {
        base_address: sdram_base,
        test_passed: sdram_ok,
    });

    defmt::info!("Initializing display...");
    let mut display = crate::display_embassy::DisplayCtrl::new(&sdram, p.PH7);
    use embedded_graphics::mono_font::ascii::FONT_10X20;
    use embedded_graphics::mono_font::MonoTextStyle;
    use embedded_graphics::pixelcolor::Rgb565;
    use embedded_graphics::prelude::*;
    use embedded_graphics::text::{Alignment, Text, TextStyleBuilder};
    display.fb().clear(Rgb565::BLACK);
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::CYAN);
    let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style("gm65-scanner", Point::new(240, 400), style, center)
        .draw(&mut display.fb())
        .ok();
    Text::with_text_style("READY", Point::new(240, 420), style, center)
        .draw(&mut display.fb())
        .ok();
    defmt::info!("Display: initialized");

    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;
    let uart = usart::Uart::new_blocking(p.USART6, p.PG9, p.PG14, uart_config).unwrap();
    embassy_stm32::interrupt::USART6.disable();

    let async_uart = AsyncUart { inner: uart };
    let mut scanner = Gm65ScannerAsync::with_default_config(async_uart);

    let mut led = embassy_stm32::gpio::Output::new(
        p.PG6,
        embassy_stm32::gpio::Level::Low,
        embassy_stm32::gpio::Speed::Low,
    );

    let mut ep_out_buffer = [0u8; 256];
    let mut usb_config = usb::Config::default();
    usb_config.vbus_detection = false;
    let usb_driver = usb::Driver::new_fs(
        p.USB_OTG_FS,
        Irqs,
        p.PA12,
        p.PA11,
        &mut ep_out_buffer,
        usb_config,
    );

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
                    let _ = SCAN_CHANNEL.try_send(ScanResult { data: data.clone() });
                    let _ = DISPLAY_CHANNEL.try_send(ScanResult { data });
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
                if let Ok(result) = SCAN_CHANNEL.try_receive() {
                    let data_str = String::from_utf8_lossy(&result.data);
                    let header = "[SCAN] ";
                    let mut msg = String::from(header);
                    msg.push_str(&data_str);
                    msg.push_str("\r\n");
                    match cdc.write_packet(msg.as_bytes()).await {
                        Ok(()) => defmt::info!("USB: sent scan result"),
                        Err(_) => break,
                    }
                    continue;
                }

                if let Ok(status) = SDRAM_CHANNEL.try_receive() {
                    let mut msg = String::from("[SDRAM] base=0x");
                    let _ = write_hex(&mut msg, status.base_address as u64);
                    msg.push_str(" test=");
                    if status.test_passed {
                        msg.push_str("PASS");
                    } else {
                        msg.push_str("FAIL");
                    }
                    msg.push_str("\r\n");
                    match cdc.write_packet(msg.as_bytes()).await {
                        Ok(()) => {}
                        Err(_) => break,
                    }
                    continue;
                }

                heartbeat.next().await;
                match cdc.write_packet(b"[ALIVE] gm65-scanner ready\r\n").await {
                    Ok(()) => {}
                    Err(_) => break,
                }
            }
            defmt::info!("USB: disconnected");
            Timer::after(Duration::from_millis(100)).await;
        }
    };

    let display_task = async {
        loop {
            let result = DISPLAY_CHANNEL.receive().await;
            let data_str = core::str::from_utf8(&result.data);
            if data_str.is_ok() && result.data.len() <= 200 {
                crate::qr_display_async::render_qr_mirror(&mut display.fb(), &result.data);
            } else {
                crate::qr_display_async::render_scan_result(&mut display.fb(), &result.data);
            }
        }
    };

    embassy_futures::join::join4(usb_task, scanner_task, cdc_task, display_task).await;
}

#[cfg(feature = "scanner-async")]
fn write_hex(buf: &mut String, val: u64) {
    let hex = b"0123456789ABCDEF";
    let mut started = false;
    for i in (0..64).step_by(4).rev() {
        let digit = ((val >> i) & 0xF) as usize;
        if digit != 0 || started || i == 0 {
            started = true;
            let _ = buf.push(hex[digit] as char);
        }
    }
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

mod display_embassy {
    #[cfg(feature = "scanner-async")]
    include!("../display_embassy.rs");
}

mod qr_display_async {
    #[cfg(feature = "scanner-async")]
    include!("../qr_display_async.rs");
}
