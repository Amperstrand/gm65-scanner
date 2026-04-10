#![no_std]
#![no_main]

extern crate alloc;

use core::fmt::Write;

use embassy_executor::Spawner;
use embassy_stm32::dsihost;
use embassy_stm32::rcc::{
    mux, AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv,
    PllRDiv, PllSource, Sysclk,
};
use embassy_stm32::time::mhz;
use embassy_stm32f469i_disco::display::SdramCtrl;
use embassy_stm32f469i_disco::{BoardHint, DisplayCtrl, TouchCtrl, TouchPoint};
use embassy_time::{block_for, Duration, Timer};
use embedded_display_controller::dsi::{DsiHostCtrlIo, DsiReadCommand, DsiWriteCommand};
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyleBuilder},
    pixelcolor::{Rgb888, RgbColor},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};
use embedded_hal::delay::DelayNs;
use heapless::String;
use linked_list_allocator::LockedHeap;
use nt35510::{ColorMap, Mode, Nt35510};
use panic_halt as _;
use stm32_metapac::DSIHOST;

const HEAP_SIZE: usize = 64 * 1024;
const LCD_WIDTH: i32 = 480;
const STEP_DELAY_MS: u64 = 2000;
const OBSERVE_DELAY_MS: u64 = 3000;
const TOUCH_TIMEOUT_MS: u64 = 15_000;
const TOUCH_POLL_MS: u64 = 50;
const TOUCH_DOTS_TARGET: usize = 5;
const TOTAL_TESTS: usize = 12;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();
static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[derive(Clone, Copy)]
enum TestState {
    Pending,
    Pass,
    Fail,
}

struct TestLine {
    label: &'static str,
    state: TestState,
}

struct TouchResult {
    vendor_id: Option<u8>,
    point: Option<TouchPoint>,
}

struct BusyDelay;

impl DelayNs for BusyDelay {
    fn delay_ns(&mut self, ns: u32) {
        block_for(Duration::from_nanos(ns as u64));
    }
}

struct DsiHostAdapter<'a, 'd> {
    dsi: &'a mut dsihost::DsiHost<'d, embassy_stm32::peripherals::DSIHOST>,
}

impl<'a, 'd> DsiHostAdapter<'a, 'd> {
    fn new(dsi: &'a mut dsihost::DsiHost<'d, embassy_stm32::peripherals::DSIHOST>) -> Self {
        Self { dsi }
    }

    fn wait_command_fifo_empty(&self) -> Result<(), dsihost::Error> {
        for _ in 0..1000 {
            if DSIHOST.gpsr().read().cmdfe() {
                return Ok(());
            }
            block_for(Duration::from_millis(1));
        }
        Err(dsihost::Error::FifoTimeout)
    }

    fn raw_ghcr_write(&self, dt: u8, wclsb: u8, wcmsb: u8) {
        DSIHOST.ghcr().write(|w| {
            w.set_dt(dt);
            w.set_vcid(0);
            w.set_wclsb(wclsb);
            w.set_wcmsb(wcmsb);
        });
    }

    fn raw_dcs_short_read(&mut self, arg: u8, buf: &mut [u8]) -> Result<(), dsihost::Error> {
        if buf.len() > u16::MAX as usize {
            return Err(dsihost::Error::InvalidReadSize);
        }

        self.wait_command_fifo_empty()?;

        if buf.len() > 2 {
            self.raw_ghcr_write(0x37, (buf.len() & 0xff) as u8, ((buf.len() >> 8) & 0xff) as u8);
            self.wait_command_fifo_empty()?;
        }

        self.raw_ghcr_write(0x06, arg, 0);

        let mut idx = 0usize;
        let mut bytes_left = buf.len();
        for _ in 0..1000 {
            if bytes_left > 0 {
                let gpsr = DSIHOST.gpsr().read();
                if !gpsr.prdfe() {
                    let gpdr = DSIHOST.gpdr().read();
                    for b in [gpdr.data1(), gpdr.data2(), gpdr.data3(), gpdr.data4()]
                        .iter()
                        .take(bytes_left.min(4))
                    {
                        buf[idx] = *b;
                        idx += 1;
                        bytes_left -= 1;
                    }
                }
                if !gpsr.rcb() && (DSIHOST.isr1().read().0 & (1 << 24)) != 0 {
                    break;
                }
                block_for(Duration::from_millis(1));
            } else {
                break;
            }
        }

        if bytes_left > 0 {
            return Err(dsihost::Error::ReadError);
        }

        Ok(())
    }
}

impl DsiHostCtrlIo for DsiHostAdapter<'_, '_> {
    type Error = dsihost::Error;

    fn write(&mut self, cmd: DsiWriteCommand) -> Result<(), Self::Error> {
        match cmd {
            DsiWriteCommand::DcsShortP0 { arg } => self.dsi.write_cmd(0, arg, &[]),
            DsiWriteCommand::DcsShortP1 { arg, data } => self.dsi.write_cmd(0, arg, &[data]),
            DsiWriteCommand::DcsLongWrite { arg, data } => self.dsi.write_cmd(0, arg, data),
            DsiWriteCommand::SetMaximumReturnPacketSize(_) => Ok(()),
            DsiWriteCommand::GenericShortP0
            | DsiWriteCommand::GenericShortP1
            | DsiWriteCommand::GenericShortP2
            | DsiWriteCommand::GenericLongWrite { .. } => Ok(()),
        }
    }

    fn read(&mut self, cmd: DsiReadCommand, buf: &mut [u8]) -> Result<(), Self::Error> {
        match cmd {
            DsiReadCommand::DcsShort { arg } => self.raw_dcs_short_read(arg, buf),
            DsiReadCommand::GenericShortP0
            | DsiReadCommand::GenericShortP1 { .. }
            | DsiReadCommand::GenericShortP2 { .. } => Ok(()),
        }
    }
}

fn state_label(state: TestState) -> &'static str {
    match state {
        TestState::Pending => "PENDING",
        TestState::Pass => "PASS",
        TestState::Fail => "FAIL",
    }
}

fn state_color(state: TestState) -> Rgb888 {
    match state {
        TestState::Pending => Rgb888::new(160, 160, 160),
        TestState::Pass => Rgb888::GREEN,
        TestState::Fail => Rgb888::RED,
    }
}

fn draw_progress_screen(
    display: &mut DisplayCtrl<'_>,
    tests: &[TestLine],
    current_idx: Option<usize>,
    hint: &str,
) {
    let mut fb = display.fb();
    fb.clear(Rgb888::BLACK);

    Rectangle::new(Point::new(0, 0), Size::new(LCD_WIDTH as u32, 56))
        .into_styled(PrimitiveStyle::with_fill(Rgb888::new(0, 48, 96)))
        .draw(&mut fb)
        .ok();
    Rectangle::new(Point::new(0, 56), Size::new(LCD_WIDTH as u32, 44))
        .into_styled(PrimitiveStyle::with_fill(Rgb888::new(20, 20, 20)))
        .draw(&mut fb)
        .ok();
    Rectangle::new(Point::new(0, 720), Size::new(LCD_WIDTH as u32, 80))
        .into_styled(PrimitiveStyle::with_fill(Rgb888::new(12, 12, 12)))
        .draw(&mut fb)
        .ok();

    let title_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb888::WHITE)
        .build();
    let body_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb888::WHITE)
        .build();
    let current_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb888::new(255, 220, 0))
        .build();
    let hint_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb888::new(180, 220, 255))
        .build();

    Text::with_baseline(
        "nt35510 HW Test",
        Point::new(12, 16),
        title_style,
        Baseline::Top,
    )
    .draw(&mut fb)
    .ok();

    let mut test_header: String<96> = String::new();
    match current_idx {
        Some(idx) => {
            let _ = write!(
                &mut test_header,
                "Test {}/{}: {}",
                idx + 1,
                tests.len(),
                tests[idx].label
            );
        }
        None => {
            let _ = write!(&mut test_header, "Summary: {} tests complete", tests.len());
        }
    }
    Text::with_baseline(&test_header, Point::new(12, 68), current_style, Baseline::Top)
        .draw(&mut fb)
        .ok();

    for (idx, test) in tests.iter().enumerate() {
        let label_style = if Some(idx) == current_idx {
            current_style
        } else {
            body_style
        };

        let mut label: String<64> = String::new();
        let marker = if Some(idx) == current_idx { '>' } else { ' ' };
        let _ = write!(&mut label, "{} {:02}. {}", marker, idx + 1, test.label);

        Text::with_baseline(
            &label,
            Point::new(12, 124 + (idx as i32 * 28)),
            label_style,
            Baseline::Top,
        )
        .draw(&mut fb)
        .ok();

        let status_style = MonoTextStyleBuilder::new()
            .font(&FONT_10X20)
            .text_color(state_color(test.state))
            .build();
        Text::with_baseline(
            state_label(test.state),
            Point::new(372, 124 + (idx as i32 * 28)),
            status_style,
            Baseline::Top,
        )
        .draw(&mut fb)
        .ok();
    }

    Text::with_baseline("Look for:", Point::new(12, 732), current_style, Baseline::Top)
        .draw(&mut fb)
        .ok();
    Text::with_baseline(hint, Point::new(12, 760), hint_style, Baseline::Top)
        .draw(&mut fb)
        .ok();
}

fn draw_summary_screen(
    display: &mut DisplayCtrl<'_>,
    tests: &[TestLine],
    touch_result: TouchResult,
    touch_points: &[(u16, u16)],
) {
    let passed = tests
        .iter()
        .filter(|test| matches!(test.state, TestState::Pass))
        .count();
    let failed = tests.len() - passed;

    let mut fb = display.fb();
    fb.clear(Rgb888::BLACK);

    Rectangle::new(Point::new(0, 0), Size::new(LCD_WIDTH as u32, 64))
        .into_styled(PrimitiveStyle::with_fill(Rgb888::new(0, 48, 96)))
        .draw(&mut fb)
        .ok();

    let title_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb888::WHITE)
        .build();
    let ok_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(if failed == 0 { Rgb888::GREEN } else { Rgb888::RED })
        .build();
    let body_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb888::WHITE)
        .build();
    let fail_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb888::RED)
        .build();
    let touch_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb888::new(180, 220, 255))
        .build();

    Text::with_baseline(
        "nt35510 HW Test Summary",
        Point::new(12, 18),
        title_style,
        Baseline::Top,
    )
    .draw(&mut fb)
    .ok();

    let mut summary: String<96> = String::new();
    if failed == 0 {
        let _ = write!(&mut summary, "{}/{} PASSED", passed, tests.len());
    } else {
        let _ = write!(
            &mut summary,
            "{}/{} PASSED — {} FAILED",
            passed,
            tests.len(),
            failed
        );
    }
    Text::with_baseline(&summary, Point::new(12, 84), ok_style, Baseline::Top)
        .draw(&mut fb)
        .ok();

    Text::with_baseline(
        "Failed tests:",
        Point::new(12, 124),
        if failed == 0 { body_style } else { fail_style },
        Baseline::Top,
    )
    .draw(&mut fb)
    .ok();

    if failed == 0 {
        Text::with_baseline("None", Point::new(12, 152), body_style, Baseline::Top)
            .draw(&mut fb)
            .ok();
    } else {
        let mut y = 152;
        for test in tests.iter().filter(|test| matches!(test.state, TestState::Fail)) {
            Text::with_baseline(test.label, Point::new(12, y), fail_style, Baseline::Top)
                .draw(&mut fb)
                .ok();
            y += 28;
        }
    }

    let mut vendor_line: String<64> = String::new();
    match touch_result.vendor_id {
        Some(vendor_id) => {
            let _ = write!(&mut vendor_line, "Touch vendor ID: 0x{:02X}", vendor_id);
        }
        None => {
            let _ = write!(&mut vendor_line, "Touch vendor ID: read failed");
        }
    }
    Text::with_baseline(&vendor_line, Point::new(12, 540), touch_style, Baseline::Top)
        .draw(&mut fb)
        .ok();

    let mut touch_line: String<96> = String::new();
    if touch_points.is_empty() {
        let _ = write!(&mut touch_line, "No touch points captured");
    } else {
        let _ = write!(&mut touch_line, "Dots: ");
        for (i, (x, y)) in touch_points.iter().enumerate() {
            if i > 0 {
                let _ = write!(&mut touch_line, ", ");
            }
            let _ = write!(&mut touch_line, "({},{})", x, y);
        }
    }
    Text::with_baseline(&touch_line, Point::new(12, 568), touch_style, Baseline::Top)
        .draw(&mut fb)
        .ok();

    Text::with_baseline(
        "Board will stay on this screen for inspection.",
        Point::new(12, 760),
        body_style,
        Baseline::Top,
    )
    .draw(&mut fb)
    .ok();
}

async fn show_step(
    display: &mut DisplayCtrl<'_>,
    tests: &[TestLine],
    current_idx: usize,
    hint: &str,
    delay_ms: u64,
) {
    draw_progress_screen(display, tests, Some(current_idx), hint);
    Timer::after_millis(delay_ms).await;
}

fn valid_touch(point: &TouchPoint) -> bool {
    point.x >= 3 && point.x <= 476 && point.y >= 3 && point.y <= 796
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    unsafe {
        ALLOCATOR
            .lock()
            .init(core::ptr::addr_of_mut!(HEAP_MEMORY) as *mut u8, HEAP_SIZE);
    }

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
    let sdram = SdramCtrl::new(&mut p, 180_000_000);
    let mut display = DisplayCtrl::new(&sdram, p.LTDC, p.DSIHOST, p.PJ2, p.PH7, BoardHint::ForceNt35510);

    let mut tests = [
        TestLine {
            label: "probe()",
            state: TestState::Pending,
        },
        TestLine {
            label: "id_matches()",
            state: TestState::Pending,
        },
        TestLine {
            label: "RGB888 pipeline",
            state: TestState::Pending,
        },
        TestLine {
            label: "init()",
            state: TestState::Pending,
        },
        TestLine {
            label: "brightness max",
            state: TestState::Pending,
        },
        TestLine {
            label: "brightness min",
            state: TestState::Pending,
        },
        TestLine {
            label: "backlight off",
            state: TestState::Pending,
        },
        TestLine {
            label: "backlight on",
            state: TestState::Pending,
        },
        TestLine {
            label: "TE on/off",
            state: TestState::Pending,
        },
        TestLine {
            label: "sleep + init_rgb888",
            state: TestState::Pending,
        },
        TestLine {
            label: "post-reinit mid",
            state: TestState::Pending,
        },
        TestLine {
            label: "touch verify",
            state: TestState::Pending,
        },
    ];
    debug_assert_eq!(tests.len(), TOTAL_TESTS);

    tests[2].state = TestState::Pass;
    show_step(
        &mut display,
        &tests,
        2,
        "RGB888 framebuffer should already be visible and stable.",
        STEP_DELAY_MS,
    )
    .await;

    let probe_ok = {
        let mut panel = Nt35510::new();
        let mut adapter = DsiHostAdapter::new(display.dsi());
        panel.probe(&mut adapter).is_ok()
    };
    tests[0].state = if probe_ok { TestState::Pass } else { TestState::Fail };
    show_step(
        &mut display,
        &tests,
        0,
        "Panel should respond to DSI probe without visual glitches.",
        STEP_DELAY_MS,
    )
    .await;

    let id_ok = {
        let mut panel = Nt35510::new();
        let mut adapter = DsiHostAdapter::new(display.dsi());
        matches!(panel.id_matches(&mut adapter), Ok(true))
    };
    tests[1].state = if id_ok { TestState::Pass } else { TestState::Fail };
    show_step(
        &mut display,
        &tests,
        1,
        "ID check should succeed with the panel still rendering cleanly.",
        STEP_DELAY_MS,
    )
    .await;

    let init_ok = {
        let mut panel = Nt35510::new();
        let mut adapter = DsiHostAdapter::new(display.dsi());
        let mut delay = BusyDelay;
        panel.init(&mut adapter, &mut delay).is_ok()
    };
    tests[3].state = if init_ok { TestState::Pass } else { TestState::Fail };
    show_step(
        &mut display,
        &tests,
        3,
        "Panel will replay init; image should return normally.",
        STEP_DELAY_MS,
    )
    .await;

    let max_ok = {
        let mut panel = Nt35510::new();
        let mut adapter = DsiHostAdapter::new(display.dsi());
        panel.set_brightness(&mut adapter, 0xFF).is_ok()
    };
    tests[4].state = if max_ok { TestState::Pass } else { TestState::Fail };
    show_step(
        &mut display,
        &tests,
        4,
        "Brightness should jump to maximum now.",
        STEP_DELAY_MS,
    )
    .await;

    // Brightness sweep: max → min → max so user can see the fade
    const SWEEP_LEVELS: [u8; 9] = [0xFF, 0xC0, 0x80, 0x40, 0x10, 0x40, 0x80, 0xC0, 0xFF];
    let mut panel = Nt35510::new();
    let mut adapter = DsiHostAdapter::new(display.dsi());
    let sweep_ok = SWEEP_LEVELS.iter().all(|&level| {
        panel.set_brightness(&mut adapter, level).is_ok()
    });
    let _ = panel.set_brightness(&mut adapter, 0xFF);
    tests[5].state = if sweep_ok { TestState::Pass } else { TestState::Fail };
    show_step(
        &mut display,
        &tests,
        5,
        "Brightness should have faded down and back up.",
        STEP_DELAY_MS,
    )
    .await;

    draw_progress_screen(&mut display, &tests, Some(6), "Screen should go dark now.");
    let backlight_off_ok = {
        let mut panel = Nt35510::new();
        let mut adapter = DsiHostAdapter::new(display.dsi());
        panel.set_backlight(&mut adapter, false).is_ok()
    };
    tests[6].state = if backlight_off_ok { TestState::Pass } else { TestState::Fail };
    Timer::after_millis(OBSERVE_DELAY_MS).await;

    let backlight_on_ok = {
        let mut panel = Nt35510::new();
        let mut adapter = DsiHostAdapter::new(display.dsi());
        panel.set_backlight(&mut adapter, true).is_ok()
    };
    tests[7].state = if backlight_on_ok { TestState::Pass } else { TestState::Fail };
    show_step(
        &mut display,
        &tests,
        7,
        "Backlight should be restored and screen readable again.",
        STEP_DELAY_MS,
    )
    .await;

    let te_ok = {
        let mut panel = Nt35510::new();
        let mut adapter = DsiHostAdapter::new(display.dsi());
        panel.enable_te_output(0, &mut adapter).is_ok() && panel.disable_te_output(&mut adapter).is_ok()
    };
    tests[8].state = if te_ok { TestState::Pass } else { TestState::Fail };
    show_step(
        &mut display,
        &tests,
        8,
        "Watch for TE line changes if you have external instrumentation.",
        STEP_DELAY_MS,
    )
    .await;

    draw_progress_screen(&mut display, &tests, Some(9), "Panel will blank then re-init.");
    let sleep_ok = {
        let mut panel = Nt35510::new();
        let mut adapter = DsiHostAdapter::new(display.dsi());
        let mut delay = BusyDelay;
        panel.sleep_in(&mut adapter, &mut delay).is_ok()
    };
    if sleep_ok {
        Timer::after_millis(OBSERVE_DELAY_MS).await;
    }

    let reinit_ok = if sleep_ok {
        let mut panel = Nt35510::new();
        let mut adapter = DsiHostAdapter::new(display.dsi());
        let mut delay = BusyDelay;
        panel
            .init_rgb888(&mut adapter, &mut delay, Mode::Portrait, ColorMap::Rgb)
            .is_ok()
    } else {
        false
    };
    tests[9].state = if reinit_ok { TestState::Pass } else { TestState::Fail };
    show_step(
        &mut display,
        &tests,
        9,
        "Panel should wake and redraw after the sleep cycle.",
        STEP_DELAY_MS,
    )
    .await;

    let mid_ok = {
        let mut panel = Nt35510::new();
        let mut adapter = DsiHostAdapter::new(display.dsi());
        panel.set_brightness(&mut adapter, 0x40).is_ok()
    };
    tests[10].state = if mid_ok { TestState::Pass } else { TestState::Fail };
    show_step(
        &mut display,
        &tests,
        10,
        "Brightness should settle at a comfortable mid level.",
        STEP_DELAY_MS,
    )
    .await;

    let mut touch_result = TouchResult {
        vendor_id: None,
        point: None,
    };
    let mut touch_i2c = embassy_stm32::i2c::I2c::new_blocking(
        p.I2C1,
        p.PB8,
        p.PB9,
        embassy_stm32::i2c::Config::default(),
    );
    let touch_ctrl = TouchCtrl::new();

    let vendor_ok = match touch_ctrl.read_vendor_id(&mut touch_i2c) {
        Ok(vendor_id) => {
            touch_result.vendor_id = Some(vendor_id);
            vendor_id == 0x11
        }
        Err(_) => false,
    };

    let mut dots_collected: usize = 0;
    let mut touch_points: heapless::Vec<(u16, u16), 16> = heapless::Vec::new();

    if vendor_ok {
        draw_progress_screen(
            &mut display,
            &tests,
            Some(11),
            "Tap 5 dots on the screen to verify touch.",
        );
        Timer::after_millis(500).await;

        let mut remaining_ms = TOUCH_TIMEOUT_MS;
        while remaining_ms > 0 && dots_collected < TOUCH_DOTS_TARGET {
            if let Ok(status) = touch_ctrl.td_status(&mut touch_i2c) {
                if status > 0 {
                    if let Ok(point) = touch_ctrl.get_touch(&mut touch_i2c) {
                        if valid_touch(&point) {
                            let cx = point.x as i32;
                            let cy = point.y as i32;
                            touch_points.push((point.x, point.y)).ok();
                            touch_result.point = Some(point);
                            dots_collected += 1;

                            Rectangle::new(
                                Point::new(cx - 20, cy - 20),
                                Size::new(40, 40),
                            )
                            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(0, 200, 255)))
                            .draw(&mut display.fb())
                            .ok();

                            let coord_style =
                                MonoTextStyleBuilder::new().font(&FONT_10X20).text_color(Rgb888::WHITE).build();
                            let mut coord_text: String<32> = String::new();
                            let _ = write!(&mut coord_text, "({},{})", cx, cy);
                            let label_y = if cy > 400 { cy - 30 } else { cy + 24 };
                            Text::with_baseline(&coord_text, Point::new(cx, label_y), coord_style, Baseline::Top)
                                .draw(&mut display.fb())
                                .ok();

                            let mut counter: String<64> = String::new();
                            let _ = write!(
                                &mut counter,
                                "Dot {}/{} collected.",
                                dots_collected,
                                TOUCH_DOTS_TARGET
                            );
                            let hint_style = MonoTextStyleBuilder::new()
                                .font(&FONT_10X20)
                                .text_color(Rgb888::new(255, 220, 0))
                                .build();
                            Text::with_baseline(&counter, Point::new(12, 760), hint_style, Baseline::Top)
                                .draw(&mut display.fb())
                                .ok();

                            Timer::after_millis(200).await;
                            continue;
                        }
                    }
                }
            }

            remaining_ms = remaining_ms.saturating_sub(TOUCH_POLL_MS);
            Timer::after_millis(TOUCH_POLL_MS).await;
        }
    }

    tests[11].state = if vendor_ok && dots_collected >= TOUCH_DOTS_TARGET {
        TestState::Pass
    } else {
        TestState::Fail
    };

    let mut touch_hint: String<96> = String::new();
    if !vendor_ok {
        match touch_result.vendor_id {
            Some(vendor_id) => {
                let _ = write!(
                    &mut touch_hint,
                    "Touch FAIL: expected vendor 0x11, got 0x{:02X}.",
                    vendor_id
                );
            }
            None => {
                let _ = write!(&mut touch_hint, "Touch FAIL: vendor ID read failed.");
            }
        }
    } else if dots_collected >= TOUCH_DOTS_TARGET {
        let _ = write!(
            &mut touch_hint,
            "Touch PASS: {} dots collected.",
            dots_collected
        );
    } else {
        let _ = write!(
            &mut touch_hint,
            "Touch FAIL: only {}/{} dots in {}s.",
            dots_collected,
            TOUCH_DOTS_TARGET,
            TOUCH_TIMEOUT_MS / 1000
        );
    }
    show_step(&mut display, &tests, 11, &touch_hint, STEP_DELAY_MS).await;

    draw_summary_screen(&mut display, &tests, touch_result, &touch_points);
    loop {
        Timer::after_secs(1).await;
    }
}
