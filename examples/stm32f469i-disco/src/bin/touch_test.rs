#![no_std]
#![no_main]
#![allow(dead_code, clippy::empty_loop)]

extern crate alloc;

use cortex_m_rt::entry;
use panic_halt as _;

use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_10X20, ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::Rectangle,
    text::{Alignment, Text, TextStyleBuilder},
};

use stm32f469i_disc::{
    hal,
    hal::ltdc::{Layer, LtdcFramebuffer},
    hal::pac::{self, CorePeripherals},
    hal::prelude::*,
    hal::rcc,
    lcd, sdram,
    sdram::alt,
};

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

const FB_W: i32 = 480;
const FB_H: i32 = 800;

const FT6X06_ADDR: u8 = 0x38;
const REG_TD_STATUS: u8 = 0x02;
const REG_TOUCH1_XH: u8 = 0x03;
const REG_VENDOR_ID: u8 = 0xA8;

struct TargetRect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    label: &'static str,
}

const TARGETS: &[TargetRect] = &[
    TargetRect {
        x: 20,
        y: 100,
        w: 120,
        h: 60,
        label: "TOP-LEFT",
    },
    TargetRect {
        x: 340,
        y: 100,
        w: 120,
        h: 60,
        label: "TOP-RIGHT",
    },
    TargetRect {
        x: 180,
        y: 370,
        w: 120,
        h: 60,
        label: "CENTER",
    },
    TargetRect {
        x: 20,
        y: 640,
        w: 120,
        h: 60,
        label: "BOT-LEFT",
    },
    TargetRect {
        x: 340,
        y: 640,
        w: 120,
        h: 60,
        label: "BOT-RIGHT",
    },
    TargetRect {
        x: 130,
        y: 520,
        w: 220,
        h: 60,
        label: "SETTINGS",
    },
];

fn draw_targets(fb: &mut impl DrawTarget<Color = Rgb565>, hit_idx: Option<usize>) {
    let label_style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
    let tc = TextStyleBuilder::new().alignment(Alignment::Center).build();

    for (i, t) in TARGETS.iter().enumerate() {
        let color = if Some(i) == hit_idx {
            Rgb565::CSS_GREEN
        } else {
            Rgb565::new(0x40, 0x40, 0x40)
        };
        let border = Rectangle::new(
            Point::new(t.x - 1, t.y - 1),
            Size::new(t.w as u32 + 2, t.h as u32 + 2),
        );
        fb.fill_solid(&border, Rgb565::CSS_WHITE).ok();
        fb.fill_solid(
            &Rectangle::new(Point::new(t.x, t.y), Size::new(t.w as u32, t.h as u32)),
            color,
        )
        .ok();
        let cx = t.x + t.w / 2;
        let cy = t.y + t.h / 2;
        Text::with_text_style(t.label, Point::new(cx, cy), label_style, tc)
            .draw(fb)
            .ok();
    }
}

fn draw_status(fb: &mut impl DrawTarget<Color = Rgb565>, line1: &str, line2: &str, line3: &str) {
    let style = MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_GREEN);
    let tc = TextStyleBuilder::new().alignment(Alignment::Center).build();

    fb.fill_solid(
        &Rectangle::new(Point::new(0, FB_H - 55), Size::new(FB_W as u32, 55)),
        Rgb565::CSS_BLACK,
    )
    .ok();
    Text::with_text_style(line1, Point::new(FB_W / 2, FB_H - 50), style, tc)
        .draw(fb)
        .ok();
    Text::with_text_style(line2, Point::new(FB_W / 2, FB_H - 38), style, tc)
        .draw(fb)
        .ok();
    Text::with_text_style(line3, Point::new(FB_W / 2, FB_H - 26), style, tc)
        .draw(fb)
        .ok();
}

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
    let sysclk_hz: u32 = rcc.clocks.sysclk().raw();

    let gpioc = dp.GPIOC.split(&mut rcc);
    let gpiod = dp.GPIOD.split(&mut rcc);
    let gpioe = dp.GPIOE.split(&mut rcc);
    let gpiof = dp.GPIOF.split(&mut rcc);
    let gpiog = dp.GPIOG.split(&mut rcc);
    let gpiob = dp.GPIOB.split(&mut rcc);
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

    {
        const HEAP_SIZE: usize = 64 * 1024;
        let heap_start = sdram.mem as *mut u8;
        let fb_bytes = lcd::DisplayOrientation::Portrait.fb_size() * core::mem::size_of::<u16>();
        unsafe {
            let heap_ptr = heap_start.add(fb_bytes);
            ALLOCATOR.lock().init(heap_ptr, HEAP_SIZE);
        }
    }

    let orientation = lcd::DisplayOrientation::Portrait;
    let fb_buffer: &'static mut [u16] = unsafe {
        &mut *core::ptr::slice_from_raw_parts_mut(sdram.mem as *mut u16, orientation.fb_size())
    };
    let mut fb = LtdcFramebuffer::new(fb_buffer, orientation.width(), orientation.height());

    let (mut display_ctrl, _controller, _orient) = lcd::init_display_full(
        dp.DSI,
        dp.LTDC,
        dp.DMA2D,
        &mut rcc,
        &mut delay,
        lcd::BoardHint::ForceNt35510,
        orientation,
    );

    fb.clear(Rgb565::CSS_BLACK).ok();

    let fb_buffer = fb.into_inner();
    display_ctrl.config_layer(Layer::L1, fb_buffer, hal::ltdc::PixelFormat::RGB565);
    display_ctrl.enable_layer(Layer::L1);
    display_ctrl.reload();

    let fb_ptr = display_ctrl
        .layer_buffer_mut(Layer::L1)
        .expect("layer L1 buffer");
    let fb_buf: &'static mut [u16] = unsafe { core::mem::transmute(fb_ptr) };
    let mut fb = LtdcFramebuffer::new(fb_buf, orientation.width(), orientation.height());

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let tc = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style("TOUCH TEST", Point::new(FB_W / 2, 30), title_style, tc)
        .draw(&mut fb)
        .ok();
    Text::with_text_style(
        "Touch the rectangles",
        Point::new(FB_W / 2, 55),
        title_style,
        tc,
    )
    .draw(&mut fb)
    .ok();

    let mut fb_label = heapless::String::<16>::new();
    core::fmt::write(&mut fb_label, format_args!("FB: {}x{}", FB_W, FB_H)).ok();
    Text::with_text_style(&fb_label, Point::new(FB_W / 2, 75), title_style, tc)
        .draw(&mut fb)
        .ok();

    let pb8 = gpiob.pb8.into_alternate_open_drain::<4>();
    let pb9 = gpiob.pb9.into_alternate_open_drain::<4>();
    let mut touch_i2c = dp.I2C1.i2c(
        (pb8, pb9),
        hal::i2c::Mode::standard(100_u32.kHz()),
        &mut rcc,
    );

    let mut buf1 = [0u8; 1];
    let touch_found = touch_i2c
        .write_read(FT6X06_ADDR, &[REG_VENDOR_ID], &mut buf1)
        .is_ok()
        && buf1[0] == 0x11;

    if !touch_found {
        Text::with_text_style("NO TOUCH CTRL", Point::new(FB_W / 2, 400), title_style, tc)
            .draw(&mut fb)
            .ok();
    }

    draw_targets(&mut fb, None);

    loop {
        if !touch_found {
            continue;
        }

        let mut status_buf = [0u8; 1];
        let touched = touch_i2c
            .write_read(FT6X06_ADDR, &[REG_TD_STATUS], &mut status_buf)
            .is_ok()
            && (status_buf[0] & 0x0F) > 0;

        if touched {
            let mut coord_buf = [0u8; 4];
            if touch_i2c
                .write_read(FT6X06_ADDR, &[REG_TOUCH1_XH], &mut coord_buf)
                .is_ok()
            {
                let tx = (((coord_buf[0] & 0x0F) as u16) << 8) | (coord_buf[1] as u16);
                let ty = (((coord_buf[2] & 0x0F) as u16) << 8) | (coord_buf[3] as u16);

                let mut hit_idx: Option<usize> = None;
                for (i, t) in TARGETS.iter().enumerate() {
                    let in_x = (tx as i32) >= t.x && (tx as i32) < t.x + t.w;
                    let in_y = (ty as i32) >= t.y && (ty as i32) < t.y + t.h;
                    if in_x && in_y {
                        hit_idx = Some(i);
                        break;
                    }
                }

                fb.clear(Rgb565::CSS_BLACK).ok();

                Text::with_text_style("TOUCH TEST", Point::new(FB_W / 2, 30), title_style, tc)
                    .draw(&mut fb)
                    .ok();
                Text::with_text_style(
                    "Touch the rectangles",
                    Point::new(FB_W / 2, 55),
                    title_style,
                    tc,
                )
                .draw(&mut fb)
                .ok();

                draw_targets(&mut fb, hit_idx);

                fb.fill_solid(
                    &Rectangle::new(Point::new(tx as i32 - 6, ty as i32 - 6), Size::new(12, 12)),
                    Rgb565::CSS_YELLOW,
                )
                .ok();

                let hit_str = match hit_idx {
                    Some(i) => TARGETS[i].label,
                    None => "MISS",
                };
                let mut l1 = heapless::String::<40>::new();
                let mut l2 = heapless::String::<40>::new();
                let mut l3 = heapless::String::<40>::new();
                core::fmt::write(&mut l1, format_args!("raw: ({}, {})", tx, ty)).ok();
                core::fmt::write(&mut l2, format_args!("dot: ({}, {})", tx, ty)).ok();
                core::fmt::write(&mut l3, format_args!("hit: {}", hit_str)).ok();
                draw_status(&mut fb, &l1, &l2, &l3);
            }
        }

        let cycles_10ms = sysclk_hz / 100;
        cortex_m::asm::delay(cycles_10ms);
    }
}
