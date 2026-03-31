#![no_std]
#![no_main]
#![allow(dead_code, clippy::empty_loop)]

extern crate alloc;

use cortex_m_rt::entry;
use defmt_rtt as _;
use panic_probe as _;

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565, prelude::*};
use stm32f469i_disc::{
    hal,
    hal::ltdc::{Layer, LtdcFramebuffer},
    hal::pac::{self, CorePeripherals},
    hal::prelude::*,
    hal::rcc,
    lcd, sdram,
    sdram::alt,
};

use gm65_scanner::{Gm65Scanner, ScannerDriverSync, ScannerModel, ScannerState};

mod cdc;
mod display_utils;
mod display {
    const DISPLAY_CENTER_X: i32 = 400;
    const DISPLAY_MAX_Y: u32 = 480;
    include!("display.rs");
}
mod qr_display {
    include!("qr_display.rs");
}

use display::render_decoded_scan;
use display::render_status;

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

#[entry]
fn main() -> ! {
    defmt::info!("=== Scanner+Display+RTT firmware starting ===");

    let dp = pac::Peripherals::take().unwrap();
    let cp = CorePeripherals::take().unwrap();

    defmt::info!("Freezing clocks...");
    let mut rcc = dp.RCC.freeze(
        rcc::Config::hse(8.MHz())
            .pclk2(32.MHz())
            .sysclk(180.MHz())
            .require_pll48clk(),
    );
    let mut delay = cp.SYST.delay(&rcc.clocks);

    defmt::info!("Clocks frozen. SYSCLK={}", rcc.clocks.sysclk().raw());

    let _gpioa = dp.GPIOA.split(&mut rcc);
    let gpioc = dp.GPIOC.split(&mut rcc);
    let gpiod = dp.GPIOD.split(&mut rcc);
    let gpioe = dp.GPIOE.split(&mut rcc);
    let gpiof = dp.GPIOF.split(&mut rcc);
    let gpiog = dp.GPIOG.split(&mut rcc);
    let scanner_tx = gpiog.pg14;
    let scanner_rx = gpiog.pg9;
    let gpioh = dp.GPIOH.split(&mut rcc);
    let gpioi = dp.GPIOI.split(&mut rcc);

    defmt::info!("GPIO split complete. LCD reset...");

    let mut lcd_reset = gpioh.ph7.into_push_pull_output();
    lcd_reset.set_low();
    delay.delay_ms(20u32);
    lcd_reset.set_high();
    delay.delay_ms(10u32);

    defmt::info!("LCD reset done. Initializing SDRAM...");

    let sdram = sdram::Sdram::new(
        dp.FMC,
        sdram::sdram_pins!(gpioc, gpiod, gpioe, gpiof, gpiog, gpioh, gpioi),
        &rcc.clocks,
        &mut delay,
    );

    defmt::info!("SDRAM initialized at {:08X}", sdram.mem as u32);

    {
        const HEAP_SIZE: usize = 64 * 1024;
        let heap_start = sdram.mem as *mut u8;
        let fb_bytes = lcd::DisplayOrientation::Portrait.fb_size() * core::mem::size_of::<u16>();
        unsafe {
            let heap_ptr = heap_start.add(fb_bytes);
            ALLOCATOR.lock().init(heap_ptr, HEAP_SIZE);
        }
    }
    defmt::info!("Heap initialized in SDRAM");

    let orientation = lcd::DisplayOrientation::Portrait;
    let fb_buffer: &'static mut [u16] = unsafe {
        &mut *core::ptr::slice_from_raw_parts_mut(sdram.mem as *mut u16, orientation.fb_size())
    };
    let mut fb = LtdcFramebuffer::new(fb_buffer, orientation.width(), orientation.height());

    defmt::info!("Framebuffer allocated. Initializing display...");

    let (mut display_ctrl, _controller, _orient) = lcd::init_display_full(
        dp.DSI,
        dp.LTDC,
        dp.DMA2D,
        &mut rcc,
        &mut delay,
        lcd::BoardHint::ForceNt35510,
        orientation,
    );

    defmt::info!("Display controller created. Configuring layer...");

    fb.clear(Rgb565::CSS_BLACK).ok();
    render_status(&mut fb, "Booting...");

    let fb_buffer = fb.into_inner();
    display_ctrl.config_layer(Layer::L1, fb_buffer, hal::ltdc::PixelFormat::RGB565);
    display_ctrl.enable_layer(Layer::L1);
    display_ctrl.reload();

    let fb_ptr = display_ctrl
        .layer_buffer_mut(Layer::L1)
        .expect("layer L1 buffer");
    let fb_buf: &'static mut [u16] = unsafe { core::mem::transmute(fb_ptr) };
    let mut fb = LtdcFramebuffer::new(fb_buf, orientation.width(), orientation.height());

    defmt::info!("Display fully initialized!");

    render_status(&mut fb, "Scanner init...");

    defmt::info!("Initializing QR scanner (USART6)...");
    let baud = 115200;
    let uart = dp
        .USART6
        .serial((scanner_tx, scanner_rx), baud.bps(), &mut rcc)
        .unwrap();
    let mut scanner = Gm65Scanner::with_default_config(uart);

    let mut model_str: &str = "Unknown";
    let scanner_connected = match scanner.init() {
        Ok(model) => {
            defmt::info!("QR scanner ready at {} bps: {}", baud, model);
            model_str = match model {
                ScannerModel::Gm65 => "GM65",
                ScannerModel::M3Y => "M3Y",
                ScannerModel::Generic => "Generic",
                ScannerModel::Unknown => "Unknown",
            };
            true
        }
        Err(e) => {
            defmt::warn!("QR scanner init failed: {}", e);
            false
        }
    };

    defmt::info!(
        "Entering main loop. Scanner connected: {}",
        scanner_connected
    );

    if scanner_connected {
        if let Some(settings) = scanner.get_scanner_settings() {
            display::render_scanner_settings(&mut fb, settings);
        } else {
            display::render_home(&mut fb, true, model_str);
        }
    } else {
        display::render_home(&mut fb, false, model_str);
    }

    let auto_scan: bool = scanner_connected;
    let mut scan_count: u32 = 0;

    loop {
        if auto_scan && !scanner.data_ready() && scanner.state() == ScannerState::Ready {
            let _ = scanner.trigger_scan();
        }

        if !scanner.data_ready() {
            for _ in 0..8 {
                if let Some(data) = scanner.try_read_scan() {
                    scan_count += 1;
                    defmt::info!("Scan #{} received: {} bytes", scan_count, data.len());
                    let payload = gm65_scanner::decode_payload(&data);
                    render_decoded_scan(&mut fb, &payload);
                    if data.len() <= 200 && core::str::from_utf8(&data).is_ok() {
                        qr_display::render_qr_mirror(&mut fb, &data);
                    }
                    break;
                }
            }
        }
    }
}
