//! Build: cargo build --release --target thumbv7em-none-eabihf --bin async_display_test --no-default-features --features scanner-async

#![no_std]
#![no_main]

use embassy_stm32::rcc::{
    mux, AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv,
    PllQDiv, PllRDiv, PllSource, Sysclk,
};
use embassy_stm32::Config;
use embassy_stm32f469i_disco::{display::SdramCtrl, DisplayCtrl, FB_HEIGHT, FB_WIDTH};
use embassy_time::{Duration, Ticker};
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyleBuilder},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{rectangle::Rectangle, PrimitiveStyle},
    text::{Alignment, Text, TextStyleBuilder},
};
use panic_halt as _;

#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn LTDC() { cortex_m::asm::nop(); }
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn LTDC_ER() { cortex_m::asm::nop(); }
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DSI() { cortex_m::asm::nop(); }
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DSIHOST() { cortex_m::asm::nop(); }
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DMA2D() { cortex_m::asm::nop(); }
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn FMC() { cortex_m::asm::nop(); }

fn delay_ms(ms: u32) {
    let sysclk = 180_000_000;
    let ticks_per_ms = sysclk / 1000;
    for _ in 0..ms {
        cortex_m::asm::delay(ticks_per_ms);
    }
}

fn blink_step(pin: &mut embassy_stm32::gpio::Output<'_>, step: u32) {
    // N short pulses, then a long pause. Count the pulses = step number.
    for _ in 0..step {
        pin.set_high();
        delay_ms(80);
        pin.set_low();
        delay_ms(80);
    }
    delay_ms(800);
}

#[embassy_executor::main]
async fn main(_spawner: embassy_executor::Spawner) {
    let mut config = Config::default();
    config.rcc.hse = Some(Hse {
        freq: embassy_stm32::time::mhz(8),
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
    config.rcc.sys = Sysclk::PLL1_P;
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV4;
    config.rcc.apb2_pre = APBPrescaler::DIV2;

    let p = embassy_stm32::init(config);

    let mut led = embassy_stm32::gpio::Output::new(
        unsafe { p.PG6.clone_unchecked() },
        embassy_stm32::gpio::Level::Low,
        embassy_stm32::gpio::Speed::Low,
    );

    // Step 1: MCU boots
    blink_step(&mut led, 1);

    // Step 2: SDRAM
    let sdram = SdramCtrl::new(
        &mut unsafe { embassy_stm32::Peripherals::steal() },
        180_000_000,
    );
    blink_step(&mut led, 2);

    // STEP 3: Configure PLLSAIDIVR for LTDC pixel clock
    // PLLSAI_R = 384MHz/7 = 54.86MHz, DIV2 → 27.43MHz pixel clock
    // RM0090 §6.3.26: DCKCFGR[17:16] PLLSAIDIVR: 00=÷1, 01=÷2, 10=÷4, 11=÷8
    unsafe {
        const RCC_BASE: usize = 0x4002_3800;
        const DCKCFGR: usize = RCC_BASE + 0x10C;
        let dckcfgr = DCKCFGR as *mut u32;
        let current = core::ptr::read_volatile(dckcfgr);
        let new_val = (current & !(0b11 << 16)) | (0b01 << 16); // DIV2 → 27.43MHz
        core::ptr::write_volatile(dckcfgr, new_val);
    }
    blink_step(&mut led, 3);

    let mut display = DisplayCtrl::new(
        &sdram,
        unsafe { p.PH7.clone_unchecked() },
        embassy_stm32f469i_disco::BoardHint::ForceNt35510,
    );
    blink_step(&mut led, 4);

    let mut fb = display.fb();
    fb.fill_solid(
        &embedded_graphics::primitives::Rectangle::new(
            Point::new(0, 0),
            Size::new(FB_WIDTH as u32, FB_HEIGHT as u32),
        ),
        Rgb565::CSS_NAVY,
    )
    .ok();
    blink_step(&mut led, 5);

    let title_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::WHITE)
        .build();
    let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
    let cx = FB_WIDTH as i32 / 2;
    Text::with_text_style("DISPLAY WORKS!", Point::new(cx, 300), title_style, center)
        .draw(&mut fb)
        .ok();
    blink_step(&mut led, 6);

    let mut ticker = Ticker::every(Duration::from_secs(1));
    let mut on = false;
    loop {
        ticker.next().await;
        let color = if on { Rgb565::CSS_RED } else { Rgb565::CSS_GREEN };
        Rectangle::new(Point::new(100, 500), Size::new(280, 100))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(&mut fb)
            .ok();
        on = !on;
    }
}
