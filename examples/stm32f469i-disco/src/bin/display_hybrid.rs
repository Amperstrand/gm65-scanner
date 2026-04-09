#![no_std]
#![no_main]

extern crate alloc;

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::rcc::{
    mux, AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllRDiv, PllSource, Sysclk,
};
use embassy_stm32::time::mhz;
use embedded_graphics::{pixelcolor::Rgb888, prelude::*, primitives::{PrimitiveStyle, Rectangle}};
use embassy_stm32f469i_disco::display::SdramCtrl;
use embassy_time::Timer;
use linked_list_allocator::LockedHeap;
use {defmt_rtt as _, panic_probe as _};

#[global_allocator]
static mut HEAP: LockedHeap = LockedHeap::empty();

const HEAP_SIZE: usize = 64 * 1024;
static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

const LCD_X_SIZE: i32 = 480;
const LCD_Y_SIZE: i32 = 800;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    unsafe {
        HEAP.lock().init(core::ptr::addr_of_mut!(HEAP_MEMORY) as *mut u8, HEAP_SIZE);
    }

    // ── Clock config: identical to verified working dsi_bsp.rs ──
    let mut config = embassy_stm32::Config::default();
    config.rcc.sys = Sysclk::PLL1_P;
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV4;
    config.rcc.apb2_pre = APBPrescaler::DIV2;

    config.rcc.hse = Some(Hse {
        freq: mhz(8),
        mode: HseMode::Oscillator,
    });
    config.rcc.pll_src = PllSource::HSE;

    config.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV8,
        mul: PllMul::MUL360,
        divp: Some(PllPDiv::DIV2),
        divq: Some(PllQDiv::DIV7),
        divr: Some(PllRDiv::DIV6),
    });

    config.rcc.pllsai = Some(Pll {
        prediv: PllPreDiv::DIV8,
        mul: PllMul::MUL384,
        divp: None,
        divq: Some(PllQDiv::DIV8),
        divr: Some(PllRDiv::DIV7),
    });

    config.rcc.mux.clk48sel = mux::Clk48sel::PLLSAI1_Q;

    let mut p = embassy_stm32::init(config);
    info!("display_hybrid: starting (portrait 480x800)");

    // ── SDRAM init (must be before moving peripherals out of p) ──
    let sdram = SdramCtrl::new(&mut p, 180_000_000);
    info!("display_hybrid: SDRAM initialized");

    // ── GPIO ──
    let mut led = Output::new(p.PG6, Level::High, Speed::Low);

    let mut display = embassy_stm32f469i_disco::DisplayCtrl::new(
        &sdram,
        p.LTDC,
        p.DSIHOST,
        p.PJ2,
        p.PH7,
        embassy_stm32f469i_disco::BoardHint::ForceNt35510,
    );
    info!("display_hybrid: DisplayCtrl::new() complete");

    let mut fb = display.fb();
    fb.clear(Rgb888::BLACK);

    let rows_per_band = LCD_Y_SIZE / 4;
    let colors = [Rgb888::new(255, 0, 0), Rgb888::new(0, 255, 0), Rgb888::new(0, 0, 255), Rgb888::new(255, 255, 255)];

    for (band, color) in colors.iter().enumerate() {
        let y = band as i32 * rows_per_band;
        let height = if band == 3 { LCD_Y_SIZE - y } else { rows_per_band };
        Rectangle::new(Point::new(0, y), Size::new(LCD_X_SIZE as u32, height as u32))
            .into_styled(PrimitiveStyle::with_fill(*color))
            .draw(&mut fb)
            .unwrap();
    }

    info!("display_hybrid: framebuffer color bands drawn");

    loop {
        led.set_low();
        Timer::after_millis(1000).await;

        led.set_high();
        Timer::after_millis(1000).await;
    }
}
