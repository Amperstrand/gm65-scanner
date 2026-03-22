#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use panic_probe as _;

use stm32f469i_disc::{
    hal::gpio::alt::fmc as alt,
    hal::ltdc::{Layer, LtdcFramebuffer, PixelFormat},
    hal::pac::{self, CorePeripherals},
    hal::prelude::*,
    hal::rcc,
    lcd, sdram,
};

/// LCD GRAM vs SDRAM retention test.
///
/// Displays a solid red frame for 3 seconds, then clears the SDRAM
/// framebuffer to solid black WITHOUT updating the LCD.
///
/// After power-cycle:
/// - Red ghost → LCD internal GRAM retained the frame
/// - Black / no ghost → SDRAM was the source
/// - Noise → neither retained long enough

#[entry]
fn main() -> ! {
    let dp = pac::Peripherals::take().unwrap();
    let cp = CorePeripherals::take().unwrap();

    let mut rcc = dp.RCC.freeze(
        rcc::Config::hse(8.MHz())
            .pclk2(32.MHz())
            .sysclk(180.MHz())
            .require_pll48clk(),
    );
    let mut delay = cp.SYST.delay(&rcc.clocks);

    let gpioc = dp.GPIOC.split(&mut rcc);
    let gpiod = dp.GPIOD.split(&mut rcc);
    let gpioe = dp.GPIOE.split(&mut rcc);
    let gpiof = dp.GPIOF.split(&mut rcc);
    let gpiog = dp.GPIOG.split(&mut rcc);
    let gpioh = dp.GPIOH.split(&mut rcc);
    let gpioi = dp.GPIOI.split(&mut rcc);

    let mut lcd_reset = gpioh.ph7.into_push_pull_output();
    lcd_reset.set_low();
    delay.delay_ms(20u32);
    lcd_reset.set_high();
    delay.delay_ms(10u32);

    let sdram = sdram::Sdram::new(
        dp.FMC,
        sdram::sdram_pins!(gpioc, gpiod, gpioe, gpiof, gpiog, gpioh, gpioi),
        &rcc.clocks,
        &mut delay,
    );

    let (mut display_ctrl, _lcd_controller) = lcd::init_display_full(
        dp.DSI,
        dp.LTDC,
        dp.DMA2D,
        &mut rcc,
        &mut delay,
        lcd::BoardHint::Unknown,
        PixelFormat::RGB565,
    );

    let fb_ptr = sdram.mem as *mut u16;
    let fb: &'static mut [u16] = unsafe { core::slice::from_raw_parts_mut(fb_ptr, lcd::FB_SIZE) };
    display_ctrl.config_layer(Layer::L1, fb, PixelFormat::RGB565);
    display_ctrl.enable_layer(Layer::L1);
    display_ctrl.reload();

    let fb_ptr = display_ctrl
        .layer_buffer_mut(Layer::L1)
        .expect("layer L1 buffer");
    let fb_buf: &'static mut [u16] = unsafe { core::mem::transmute(fb_ptr) };

    let raw_fb = fb_buf.as_mut_ptr();
    let fb_len = fb_buf.len();

    let mut fb = LtdcFramebuffer::new(fb_buf, lcd::WIDTH, lcd::HEIGHT);

    // Phase 1: Fill with solid RED (RGB565: 0xF800)
    use embedded_graphics::{
        pixelcolor::Rgb565,
        prelude::*,
        primitives::{PrimitiveStyle, Rectangle},
    };
    let red = Rgb565::new(255, 0, 0);
    Rectangle::new(
        Point::new(0, 0),
        Size::new(lcd::WIDTH as u32, lcd::HEIGHT as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(red))
    .draw(&mut fb)
    .unwrap();
    display_ctrl.reload();

    defmt::info!("=== GRAM RETENTION TEST ===");
    defmt::info!("Phase 1: RED frame displayed. Wait 3 seconds...");
    delay.delay_ms(3000u32);

    // Phase 2: Clear SDRAM framebuffer to BLACK (0x0000)
    // Do NOT call display_ctrl.reload() — the LCD keeps showing red
    for i in 0..fb_len {
        unsafe {
            *raw_fb.add(i) = 0x0000;
        };
    }
    defmt::info!("Phase 2: SDRAM cleared to BLACK (LCD NOT updated)");
    defmt::info!("LCD should still show RED from its GRAM");
    defmt::info!("");
    defmt::info!("NOW: power-cycle the board and observe:");
    defmt::info!("  RED ghost  = LCD GRAM retention (H1 confirmed)");
    defmt::info!("  BLACK      = SDRAM retention (H2 confirmed)");
    defmt::info!("  NOISE      = neither retained");
    defmt::info!("");
    defmt::info!("Idling forever. USB not initialized for this test.");

    loop {
        cortex_m::asm::nop();
    }
}
