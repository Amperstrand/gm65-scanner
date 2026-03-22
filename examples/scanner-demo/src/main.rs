//! GM65 Scanner Demo for STM32F469I-Discovery
//!
//! This example demonstrates the GM65 QR scanner using the async driver
//! with Embassy. It displays scan results on the LCD screen.
//!
//! # Features
//!
//! - `hil-tests` — Run HIL tests instead of normal demo loop
//!
//! # Hardware
//!
//! - USART1: TX=PA9, RX=PA10 (scanner serial)
//! - PA1: Trigger pin (active low pulse)
//! - PG6: LED
//! - LCD: DSI/LTDC display

#![no_main]
#![no_std]

extern crate alloc;

use alloc::format;
use alloc_cortex_m::CortexMHeap;
use core::convert::Infallible;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::dsihost::DsiHost;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::ltdc::Ltdc;
use embassy_stm32::pac::dsihost::regs::{Ier0, Ier1};
use embassy_stm32::pac::ltdc::vals::{Bf1, Bf2, Depol, Hspol, Imr, Pcpol, Pf, Vspol};
use embassy_stm32::pac::{DSIHOST, LTDC};
use embassy_stm32::rcc::{
    AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllRDiv, PllSource, Sysclk,
};
use embassy_stm32::time::mhz;
use embassy_stm32::usart::{BufferedUart, Config as UartConfig};
use embassy_stm32::{bind_interrupts, peripherals, usart};
use embassy_time::{Duration, Timer, block_for};
use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    text::Text,
};
use gm65_scanner::{decode_payload, Gm65ScannerAsync, ScanMode, ScannerConfig, ScannerDriver, ScannerModel, ScannerState};
use panic_probe as _;

#[global_allocator]
static ALLOCATOR: CortexMHeap = CortexMHeap::empty();

const LCD_W: usize = 800;
const LCD_H: usize = 480;
const FB_W: usize = 360;
const FB_H: usize = 140;
const FB_X: u16 = 20;
const FB_Y: u16 = 20;

static mut FRAMEBUFFER: [u32; FB_W * FB_H] = [0; FB_W * FB_H];

bind_interrupts!(struct Irqs {
    USART1 => usart::BufferedInterruptHandler<peripherals::USART1>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    unsafe {
        ALLOCATOR.init(cortex_m_rt::heap_start() as usize, 48 * 1024);
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
        divq: None,
        divr: Some(PllRDiv::DIV7),
    });

    let p = embassy_stm32::init(config);

    let mut _led = Output::new(p.PG6, Level::High, Speed::Low);
    init_display(p.LTDC, p.DSIHOST, p.PJ2, p.PH7);

    let _trigger = Output::new(p.PA1, Level::High, Speed::Low);
    let mut uart_cfg = UartConfig::default();
    uart_cfg.baudrate = 9_600;
    let mut tx_buf = [0u8; 256];
    let mut rx_buf = [0u8; 512];
    let uart = BufferedUart::new(p.USART1, p.PA10, p.PA9, &mut tx_buf, &mut rx_buf, Irqs, uart_cfg).unwrap();

    let scanner_cfg = ScannerConfig {
        model: ScannerModel::Unknown,
        baud_rate: 9600,
        mode: ScanMode::CommandTriggered,
        raw_mode: true,
    };

    // Use library's Gm65ScannerAsync instead of custom implementation
    let mut scanner = Gm65ScannerAsync::new(uart, scanner_cfg);

    // =========================================================================
    // HIL Test Mode
    // =========================================================================
    #[cfg(feature = "hil-tests")]
    {
        use gm65_scanner::driver::async_hil_tests;
        
        defmt::info!("==== HIL TEST MODE ENABLED ====");
        render_status("HIL TEST MODE", "Running tests...");
        
        let results = async_hil_tests::run_hil_tests(&mut scanner).await;
        
        let status = if results.all_passed() {
            "ALL PASSED"
        } else {
            "SOME FAILED"
        };
        let msg = format!("{}/5 {}", results.passed_count(), status);
        render_status("HIL RESULTS", &msg);
        
        defmt::info!("==== HIL TESTS DONE: {} ====", msg);
        
        // Halt after tests complete
        loop {
            cortex_m::asm::wfi();
        }
    }

    // =========================================================================
    // Normal Demo Mode
    // =========================================================================
    #[cfg(not(feature = "hil-tests"))]
    {
        let init_message = match scanner.init().await {
            Ok(model) => format!("initialized: {:?}", model),
            Err(err) => format!("init error: {:?}", err),
        };
        render_status("GM65 scanner", &init_message);
        Timer::after_millis(1000).await;

        loop {
            if !matches!(scanner.state(), ScannerState::Ready | ScannerState::ScanComplete) {
                let _ = scanner.init().await;
            }

            if scanner.trigger_scan().await.is_err() {
                render_status("GM65 scanner", "trigger failed");
                Timer::after_millis(300).await;
                continue;
            }

            render_status("Scan now...", "waiting for EOL payload");

            if let Some(payload) = scanner.read_scan().await {
                let decoded = decode_payload(&payload);
                let txt = decoded.as_str().unwrap_or("<binary>");
                let line1 = trim_text(txt, 36);
                let line2 = format!("{} ({}B)", decoded.payload_type, payload.len());
                render_status(line1, &line2);
                Timer::after_millis(1200).await;
            } else {
                render_status("scan timeout", "no payload");
                Timer::after_millis(600).await;
            }
        }
    }
}

fn trim_text(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let idx = s.char_indices().nth(max_chars).map(|(i, _)| i).unwrap_or(s.len());
    &s[..idx]
}

fn render_status(line1: &str, line2: &str) {
    let fb_ptr = core::ptr::addr_of_mut!(FRAMEBUFFER) as *mut u32;
    let fb = unsafe { core::slice::from_raw_parts_mut(fb_ptr, FB_W * FB_H) };
    let mut target = Argb8888Target::new(fb, FB_W as u32, FB_H as u32);
    let _ = target.clear(Rgb888::BLACK);
    let style = MonoTextStyle::new(&FONT_6X10, Rgb888::GREEN);
    let _ = Text::new(line1, Point::new(12, 24), style).draw(&mut target);
    let _ = Text::new(line2, Point::new(12, 44), style).draw(&mut target);
    let _ = Text::new("USART1 PA9/PA10", Point::new(12, 72), style).draw(&mut target);
    let _ = Text::new("Trigger PA1", Point::new(12, 92), style).draw(&mut target);
    LTDC.srcr().modify(|w| w.set_imr(Imr::RELOAD));
}

struct Argb8888Target<'a> {
    buf: &'a mut [u32],
    w: u32,
    h: u32,
}

impl<'a> Argb8888Target<'a> {
    fn new(buf: &'a mut [u32], w: u32, h: u32) -> Self {
        Self { buf, w, h }
    }

    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return None;
        }
        Some((y as usize) * (self.w as usize) + (x as usize))
    }
}

impl OriginDimensions for Argb8888Target<'_> {
    fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }
}

impl DrawTarget for Argb8888Target<'_> {
    type Color = Rgb888;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if let Some(i) = self.idx(point.x, point.y) {
                let packed = ((0xFFu32) << 24) | ((color.r() as u32) << 16) | ((color.g() as u32) << 8) | (color.b() as u32);
                self.buf[i] = packed;
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        let packed = ((0xFFu32) << 24) | ((color.r() as u32) << 16) | ((color.g() as u32) << 8) | (color.b() as u32);
        for px in self.buf.iter_mut() {
            *px = packed;
        }
        Ok(())
    }
}

fn init_display(
    ltdc_p: embassy_stm32::Peri<'static, peripherals::LTDC>,
    dsi_p: embassy_stm32::Peri<'static, peripherals::DSIHOST>,
    te_pin: embassy_stm32::Peri<'static, peripherals::PJ2>,
    reset_pin: embassy_stm32::Peri<'static, peripherals::PH7>,
) {
    let mut reset = Output::new(reset_pin, Level::Low, Speed::High);
    block_for(Duration::from_millis(20));
    reset.set_high();
    block_for(Duration::from_millis(140));

    let mut ltdc = Ltdc::new(ltdc_p);
    let mut dsi = DsiHost::new(dsi_p, te_pin);

    dsi.disable_wrapper_dsi();
    dsi.disable();
    DSIHOST.pctlr().modify(|w| {
        w.set_cke(false);
        w.set_den(false)
    });
    DSIHOST.wrpcr().modify(|w| w.set_pllen(false));
    DSIHOST.wrpcr().write(|w| w.set_regen(true));

    for _ in 0..1000 {
        if DSIHOST.wisr().read().rrs() {
            break;
        }
        block_for(Duration::from_millis(1));
    }

    DSIHOST.wrpcr().modify(|w| {
        w.set_pllen(true);
        w.set_ndiv(125);
        w.set_idf(2);
        w.set_odf(0);
    });
    for _ in 0..1000 {
        if DSIHOST.wisr().read().pllls() {
            break;
        }
        block_for(Duration::from_millis(1));
    }

    DSIHOST.pctlr().write(|w| {
        w.set_cke(true);
        w.set_den(true);
    });
    DSIHOST.clcr().modify(|w| {
        w.set_dpcc(true);
        w.set_acr(false);
    });
    DSIHOST.pconfr().modify(|w| w.set_nl(1));
    DSIHOST.ccr().modify(|w| w.set_txeckdiv(4));
    DSIHOST.wpcr0().modify(|w| w.set_uix4(8));
    DSIHOST.ier0().write_value(Ier0(0));
    DSIHOST.ier1().write_value(Ier1(0));
    DSIHOST.pcr().modify(|w| w.set_btae(true));

    const HSA: u16 = 2;
    const HBP: u16 = 34;
    const HFP: u16 = 34;
    const VSA: u16 = 120;
    const VBP: u16 = 150;
    const VFP: u16 = 150;
    const HACT: u16 = LCD_W as u16;
    const VACT: u16 = LCD_H as u16;

    DSIHOST.mcr().modify(|w| w.set_cmdm(false));
    DSIHOST.wcfgr().modify(|w| {
        w.set_dsim(false);
        w.set_colmux(0x00);
    });
    DSIHOST.vmcr().modify(|w| {
        w.set_vmt(2);
        w.set_lpce(true);
        w.set_lphfpe(true);
        w.set_lphbpe(true);
        w.set_lpvae(true);
        w.set_lpvfpe(true);
        w.set_lpvbpe(true);
        w.set_lpvsae(true);
    });
    DSIHOST.vpcr().modify(|w| w.set_vpsize(HACT));
    DSIHOST.vccr().modify(|w| w.set_numc(0));
    DSIHOST.vnpcr().modify(|w| w.set_npsize(0x0FFF));
    DSIHOST.lvcidr().modify(|w| w.set_vcid(0));
    DSIHOST.lpcr().modify(|w| {
        w.set_dep(false);
        w.set_hsp(false);
        w.set_vsp(false);
    });
    DSIHOST.lcolcr().modify(|w| w.set_colc(0x00));
    DSIHOST.vhsacr().modify(|w| w.set_hsa(4));
    DSIHOST.vhbpcr().modify(|w| w.set_hbp(77));
    DSIHOST.vlcr().modify(|w| w.set_hline(1982));
    DSIHOST.vvsacr().modify(|w| w.set_vsa(VSA));
    DSIHOST.vvbpcr().modify(|w| w.set_vbp(VBP));
    DSIHOST.vvfpcr().modify(|w| w.set_vfp(VFP));
    DSIHOST.vvacr().modify(|w| w.set_va(VACT));
    DSIHOST.lpmcr().modify(|w| {
        w.set_lpsize(16);
        w.set_vlpsize(0);
    });
    DSIHOST.cltcr().modify(|w| {
        w.set_hs2lp_time(35);
        w.set_lp2hs_time(35);
    });
    DSIHOST.dltcr().modify(|w| {
        w.set_hs2lp_time(35);
        w.set_lp2hs_time(35);
        w.set_mrd_time(0);
    });
    DSIHOST.pconfr().modify(|w| w.set_sw_time(10));

    ltdc.disable();
    LTDC.gcr().modify(|w| {
        w.set_hspol(Hspol::ACTIVE_HIGH);
        w.set_vspol(Vspol::ACTIVE_HIGH);
        w.set_depol(Depol::ACTIVE_LOW);
        w.set_pcpol(Pcpol::RISING_EDGE);
    });
    LTDC.sscr().modify(|w| {
        w.set_hsw(HSA - 1);
        w.set_vsh(VSA - 1)
    });
    LTDC.bpcr().modify(|w| {
        w.set_ahbp(HSA + HBP - 1);
        w.set_avbp(VSA + VBP - 1);
    });
    LTDC.awcr().modify(|w| {
        w.set_aaw(HACT + HSA + HBP - 1);
        w.set_aah(VACT + VSA + VBP - 1);
    });
    LTDC.twcr().modify(|w| {
        w.set_totalw(HACT + HSA + HBP + HFP - 1);
        w.set_totalh(VACT + VSA + VBP + VFP - 1);
    });
    LTDC.bccr().modify(|w| {
        w.set_bcred(0);
        w.set_bcgreen(0);
        w.set_bcblue(0)
    });
    LTDC.ier().modify(|w| {
        w.set_terrie(true);
        w.set_fuie(true);
    });
    ltdc.enable();

    dsi.enable();
    dsi.enable_wrapper_dsi();

    block_for(Duration::from_millis(120));
    dsi.write_cmd(0, NT35510_WRITES_0[0], &NT35510_WRITES_0[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_1[0], &NT35510_WRITES_1[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_2[0], &NT35510_WRITES_2[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_3[0], &NT35510_WRITES_3[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_4[0], &NT35510_WRITES_4[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_5[0], &NT35510_WRITES_5[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_6[0], &NT35510_WRITES_6[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_7[0], &NT35510_WRITES_7[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_8[0], &NT35510_WRITES_8[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_9[0], &NT35510_WRITES_9[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_10[0], &NT35510_WRITES_10[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_12[0], &NT35510_WRITES_12[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_13[0], &NT35510_WRITES_13[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_14[0], &NT35510_WRITES_14[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_15[0], &NT35510_WRITES_15[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_16[0], &NT35510_WRITES_16[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_17[0], &NT35510_WRITES_17[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_18[0], &NT35510_WRITES_18[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_19[0], &NT35510_WRITES_19[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_20[0], &NT35510_WRITES_20[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_21[0], &NT35510_WRITES_21[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_22[0], &NT35510_WRITES_22[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_23[0], &NT35510_WRITES_23[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_24[0], &NT35510_WRITES_24[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_26[0], &NT35510_WRITES_26[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_37[0], &NT35510_WRITES_37[1..]).ok();
    block_for(Duration::from_millis(200));
    dsi.write_cmd(0, NT35510_MADCTL_LANDSCAPE[0], &NT35510_MADCTL_LANDSCAPE[1..]).ok();
    dsi.write_cmd(0, NT35510_CASET_LANDSCAPE[0], &NT35510_CASET_LANDSCAPE[1..]).ok();
    dsi.write_cmd(0, NT35510_RASET_LANDSCAPE[0], &NT35510_RASET_LANDSCAPE[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_27[0], &NT35510_WRITES_27[1..]).ok();
    block_for(Duration::from_millis(120));
    dsi.write_cmd(0, NT35510_WRITES_37[0], &NT35510_WRITES_37[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_31[0], &NT35510_WRITES_31[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_30[0], &NT35510_WRITES_30[1..]).ok();
    dsi.write_cmd(0, NT35510_WRITES_35[0], &NT35510_WRITES_35[1..]).ok();

    let window_x0 = FB_X;
    let window_y0 = FB_Y;
    let window_x1 = FB_X + FB_W as u16;
    let window_y1 = FB_Y + FB_H as u16;

    LTDC.layer(0).whpcr().write(|w| {
        w.set_whstpos(LTDC.bpcr().read().ahbp() + 1 + window_x0);
        w.set_whsppos(LTDC.bpcr().read().ahbp() + window_x1);
    });
    LTDC.layer(0).wvpcr().write(|w| {
        w.set_wvstpos(LTDC.bpcr().read().avbp() + 1 + window_y0);
        w.set_wvsppos(LTDC.bpcr().read().avbp() + window_y1);
    });
    LTDC.layer(0).pfcr().write(|w| w.set_pf(Pf::ARGB8888));
    LTDC.layer(0).dccr().modify(|w| {
        w.set_dcblue(0);
        w.set_dcgreen(0);
        w.set_dcred(0);
        w.set_dcalpha(0);
    });
    LTDC.layer(0).cacr().write(|w| w.set_consta(255));
    LTDC.layer(0).bfcr().write(|w| {
        w.set_bf1(Bf1::CONSTANT);
        w.set_bf2(Bf2::CONSTANT);
    });

    let fb_addr = core::ptr::addr_of!(FRAMEBUFFER) as *const u32 as u32;
    LTDC.layer(0).cfbar().write(|w| w.set_cfbadd(fb_addr));
    LTDC.layer(0).cfblr().write(|w| {
        w.set_cfbp((FB_W as u16) * 4);
        w.set_cfbll(((FB_W as u16) * 4) + 3);
    });
    LTDC.layer(0).cfblnr().write(|w| w.set_cfblnbr(FB_H as u16));
    LTDC.layer(0).cr().modify(|w| w.set_len(true));
    LTDC.srcr().modify(|w| w.set_imr(Imr::RELOAD));
}

const NT35510_WRITES_0: &[u8] = &[0xF0, 0x55, 0xAA, 0x52, 0x08, 0x01];
const NT35510_WRITES_1: &[u8] = &[0xB0, 0x03, 0x03, 0x03];
const NT35510_WRITES_2: &[u8] = &[0xB6, 0x46, 0x46, 0x46];
const NT35510_WRITES_3: &[u8] = &[0xB1, 0x03, 0x03, 0x03];
const NT35510_WRITES_4: &[u8] = &[0xB7, 0x36, 0x36, 0x36];
const NT35510_WRITES_5: &[u8] = &[0xB2, 0x00, 0x00, 0x02];
const NT35510_WRITES_6: &[u8] = &[0xB8, 0x26, 0x26, 0x26];
const NT35510_WRITES_7: &[u8] = &[0xBF, 0x01];
const NT35510_WRITES_8: &[u8] = &[0xB3, 0x09, 0x09, 0x09];
const NT35510_WRITES_9: &[u8] = &[0xB9, 0x36, 0x36, 0x36];
const NT35510_WRITES_10: &[u8] = &[0xB5, 0x08, 0x08, 0x08];
const NT35510_WRITES_12: &[u8] = &[0xBA, 0x26, 0x26, 0x26];
const NT35510_WRITES_13: &[u8] = &[0xBC, 0x00, 0x80, 0x00];
const NT35510_WRITES_14: &[u8] = &[0xBD, 0x00, 0x80, 0x00];
const NT35510_WRITES_15: &[u8] = &[0xBE, 0x00, 0x50];
const NT35510_WRITES_16: &[u8] = &[0xF0, 0x55, 0xAA, 0x52, 0x08, 0x00];
const NT35510_WRITES_17: &[u8] = &[0xB1, 0xFC, 0x00];
const NT35510_WRITES_18: &[u8] = &[0xB6, 0x03];
const NT35510_WRITES_19: &[u8] = &[0xB5, 0x51];
const NT35510_WRITES_20: &[u8] = &[0x00, 0x00, 0xB7];
const NT35510_WRITES_21: &[u8] = &[0xB8, 0x01, 0x02, 0x02, 0x02];
const NT35510_WRITES_22: &[u8] = &[0xBC, 0x00, 0x00, 0x00];
const NT35510_WRITES_23: &[u8] = &[0xCC, 0x03, 0x00, 0x00];
const NT35510_WRITES_24: &[u8] = &[0xBA, 0x01];
const NT35510_WRITES_26: &[u8] = &[0x35, 0x00];
const NT35510_WRITES_27: &[u8] = &[0x11, 0x00];
const NT35510_WRITES_30: &[u8] = &[0x29, 0x00];
const NT35510_WRITES_31: &[u8] = &[0x51, 0x7F];
const NT35510_WRITES_35: &[u8] = &[0x2C, 0x00];
const NT35510_WRITES_37: &[u8] = &[0x3A, 0x77];
const NT35510_MADCTL_LANDSCAPE: &[u8] = &[0x36, 0x60];
const NT35510_CASET_LANDSCAPE: &[u8] = &[0x2A, 0x00, 0x00, 0x03, 0x1F];
const NT35510_RASET_LANDSCAPE: &[u8] = &[0x2B, 0x00, 0x00, 0x01, 0xDF];
