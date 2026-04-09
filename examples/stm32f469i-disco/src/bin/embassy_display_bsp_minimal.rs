#![no_std]
#![no_main]

use defmt::info;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::dsihost::DsiHost;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::ltdc::Ltdc;
use embassy_stm32::rcc::{
    AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllRDiv,
    PllSource, Sysclk,
};
use embassy_stm32f469i_disco::display::SdramCtrl;
use embedded_display_controller::dsi::{DsiHostCtrlIo, DsiReadCommand, DsiWriteCommand};
use embassy_time::{Duration, Timer, block_for};
use nt35510::Nt35510;
use otm8009a::{ColorMap as OtmColorMap, FrameRate as OtmFrameRate, Mode as OtmMode, Otm8009A, Otm8009AConfig};
use panic_probe as _;

const DSI_BASE: usize = 0x4001_6C00;
const LTDC_BASE: usize = 0x4001_6800;
const RCC_BASE: usize = 0x4002_3800;

const LCD_X_SIZE: u16 = 480;
const LCD_Y_SIZE: u16 = 800;
const FB_PIXELS: usize = LCD_X_SIZE as usize * LCD_Y_SIZE as usize;

const DSI_WCFGR: usize = 0x400;
const DSI_WCR: usize = 0x404;
const DSI_WISR: usize = 0x40C;
const DSI_PCTLR: usize = 0xA0;
const DSI_CLCR: usize = 0x94;
const DSI_PCONFR: usize = 0xA4;
const DSI_CCR: usize = 0x08;
const DSI_WPCR0: usize = 0x418;
const DSI_WPCR1: usize = 0x41C;
const DSI_IER0: usize = 0xC4;
const DSI_IER1: usize = 0xC8;
const DSI_PCR: usize = 0x2C;
const DSI_MCR: usize = 0x34;
const DSI_VMCR: usize = 0x38;
const DSI_VPCR: usize = 0x3C;
const DSI_VCCR: usize = 0x40;
const DSI_VNPCR: usize = 0x44;
const DSI_LVCIDR: usize = 0x0C;
const DSI_LPCR: usize = 0x14;
const DSI_LCOLCR: usize = 0x10;
const DSI_VHSACR: usize = 0x48;
const DSI_VHBPCR: usize = 0x4C;
const DSI_VLCR: usize = 0x50;
const DSI_VVSACR: usize = 0x54;
const DSI_VVBPCR: usize = 0x58;
const DSI_VVFPCR: usize = 0x5C;
const DSI_VVACR: usize = 0x60;
const DSI_LPMCR: usize = 0x18;
const DSI_CLTCR: usize = 0x98;
const DSI_DLTCR: usize = 0x9C;
const DSI_WRPCR: usize = 0x430;
const DSI_CMCR: usize = 0x68;
const DSI_GHCR: usize = 0x6C;
const DSI_GPDR: usize = 0x70;
const DSI_GPSR: usize = 0x74;

const LTDC_SSCR: usize = 0x08;
const LTDC_BPCR: usize = 0x0C;
const LTDC_AWCR: usize = 0x10;
const LTDC_TWCR: usize = 0x14;
const LTDC_GCR: usize = 0x18;
const LTDC_SRCR: usize = 0x24;
const LTDC_BCCR: usize = 0x2C;
const LTDC_L1_BASE: usize = 0x84;

const RCC_PLLSAICFGR: usize = 0x88;
const RCC_DCKCFGR: usize = 0x8C;
const RCC_CR: usize = 0x00;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Nt35510,
    Otm8009a,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DsiIoError {
    Read,
}

struct RawDsi;

#[inline(always)]
unsafe fn reg32(base: usize, offset: usize) -> u32 {
    core::ptr::read_volatile((base + offset) as *const u32)
}

#[inline(always)]
unsafe fn reg32_write(base: usize, offset: usize, val: u32) {
    core::ptr::write_volatile((base + offset) as *mut u32, val)
}

#[inline(always)]
unsafe fn reg32_set(base: usize, offset: usize, val: u32) {
    reg32_write(base, offset, reg32(base, offset) | val)
}

#[inline(always)]
unsafe fn reg32_clear(base: usize, offset: usize, val: u32) {
    reg32_write(base, offset, reg32(base, offset) & !val)
}

#[inline(always)]
unsafe fn reg32_modify(base: usize, offset: usize, f: impl FnOnce(u32) -> u32) {
    reg32_write(base, offset, f(reg32(base, offset)))
}

fn log_core_state(tag: &str) {
    unsafe {
        info!(
            "{} WISR={=u32:08x} WCR={=u32:08x} WCFGR={=u32:08x} VMCR={=u32:08x}",
            tag,
            reg32(DSI_BASE, DSI_WISR),
            reg32(DSI_BASE, DSI_WCR),
            reg32(DSI_BASE, DSI_WCFGR),
            reg32(DSI_BASE, DSI_VMCR),
        );
        info!(
            "{} VPCR={=u32:08x} VCCR={=u32:08x} VNPCR={=u32:08x} LCOLCR={=u32:08x}",
            tag,
            reg32(DSI_BASE, DSI_VPCR),
            reg32(DSI_BASE, DSI_VCCR),
            reg32(DSI_BASE, DSI_VNPCR),
            reg32(DSI_BASE, DSI_LCOLCR),
        );
        info!(
            "{} LTDC_GCR={=u32:08x} SSCR={=u32:08x} BPCR={=u32:08x} AWCR={=u32:08x}",
            tag,
            reg32(LTDC_BASE, LTDC_GCR),
            reg32(LTDC_BASE, LTDC_SSCR),
            reg32(LTDC_BASE, LTDC_BPCR),
            reg32(LTDC_BASE, LTDC_AWCR),
        );
        info!(
            "{} TWCR={=u32:08x} L1CR={=u32:08x} L1PFCR={=u32:08x} L1CFBAR={=u32:08x}",
            tag,
            reg32(LTDC_BASE, LTDC_TWCR),
            reg32(LTDC_BASE, LTDC_L1_BASE),
            reg32(LTDC_BASE, LTDC_L1_BASE + 0x10),
            reg32(LTDC_BASE, LTDC_L1_BASE + 0x28),
        );
    }
}

impl RawDsi {
    const GHCR: usize = DSI_GHCR;
    const GPDR: usize = DSI_GPDR;
    const GPSR: usize = DSI_GPSR;
    const ISR1: usize = 0xC0;

    fn wait_command_fifo_empty(&self) -> Result<(), DsiIoError> {
        for _ in 0..1000 {
            if unsafe { reg32(DSI_BASE, Self::GPSR) & (1 << 0) } != 0 {
                return Ok(());
            }
            block_for(Duration::from_millis(1));
        }
        Err(DsiIoError::Read)
    }

    fn raw_ghcr_write(&self, dt: u8, wclsb: u8, wcmsb: u8) {
        unsafe {
            reg32_write(
                DSI_BASE,
                Self::GHCR,
                (dt as u32) | ((wclsb as u32) << 8) | ((wcmsb as u32) << 16),
            );
        }
    }

    fn raw_dcs_short_read(&mut self, arg: u8, buf: &mut [u8]) -> Result<(), DsiIoError> {
        self.wait_command_fifo_empty()?;

        if buf.len() > 2 {
            self.raw_ghcr_write(0x37, (buf.len() & 0xff) as u8, ((buf.len() >> 8) & 0xff) as u8);
            self.wait_command_fifo_empty()?;
        }

        self.raw_ghcr_write(0x06, arg, 0);

        let mut idx = 0usize;
        let mut bytes_left = buf.len();
        for _ in 0..1000 {
            if bytes_left == 0 {
                break;
            }

            let gpsr = unsafe { reg32(DSI_BASE, Self::GPSR) };
            if gpsr & (1 << 3) == 0 {
                let fifoword = unsafe { reg32(DSI_BASE, Self::GPDR) };
                for b in fifoword.to_ne_bytes().iter().take(bytes_left.min(4)) {
                    buf[idx] = *b;
                    idx += 1;
                    bytes_left -= 1;
                }
            }

            if gpsr & (1 << 6) == 0 && unsafe { reg32(DSI_BASE, Self::ISR1) & (1 << 24) } != 0 {
                break;
            }

            block_for(Duration::from_millis(1));
        }

        if bytes_left == 0 {
            Ok(())
        } else {
            Err(DsiIoError::Read)
        }
    }
}

impl DsiHostCtrlIo for RawDsi {
    type Error = DsiIoError;

    fn write(&mut self, command: DsiWriteCommand) -> Result<(), Self::Error> {
        match command {
            DsiWriteCommand::DcsShortP0 { arg } => unsafe { raw_dsi_write_cmd(arg, &[]) },
            DsiWriteCommand::DcsShortP1 { arg, data } => unsafe { raw_dsi_write_cmd(arg, &[data]) },
            DsiWriteCommand::DcsLongWrite { arg, data } => unsafe { raw_dsi_write_cmd(arg, data) },
            DsiWriteCommand::SetMaximumReturnPacketSize(_) => {}
            DsiWriteCommand::GenericShortP0
            | DsiWriteCommand::GenericShortP1
            | DsiWriteCommand::GenericShortP2
            | DsiWriteCommand::GenericLongWrite { .. } => {}
        }
        Ok(())
    }

    fn read(&mut self, command: DsiReadCommand, buf: &mut [u8]) -> Result<(), Self::Error> {
        match command {
            DsiReadCommand::DcsShort { arg } => self.raw_dcs_short_read(arg, buf),
            DsiReadCommand::GenericShortP0
            | DsiReadCommand::GenericShortP1 { .. }
            | DsiReadCommand::GenericShortP2 { .. } => Err(DsiIoError::Read),
        }
    }
}

fn detect_panel() -> Panel {
    let mut raw_dsi = RawDsi;
    let mut nt = Nt35510::new();
    let mut delay = BusyDelay;
    let mut mismatch_count = 0u8;
    let mut first_mismatch: Option<u8> = None;
    let mut consistent_mismatch = true;

    for attempt in 1..=3 {
        match nt.probe(&mut raw_dsi, &mut delay) {
            Ok(()) => {
                info!("panel detect: NT35510 on attempt {}", attempt);
                return Panel::Nt35510;
            }
            Err(nt35510::Error::ProbeMismatch(id)) => {
                info!("panel detect: NT35510 mismatch attempt {} id=0x{:02x}", attempt, id);
                mismatch_count = mismatch_count.saturating_add(1);
                match first_mismatch {
                    None => first_mismatch = Some(id),
                    Some(first) if first != id => consistent_mismatch = false,
                    Some(_) => {}
                }
            }
            Err(nt35510::Error::DsiRead) => {
                info!("panel detect: NT35510 read error attempt {}", attempt);
            }
            Err(_) => {
                info!("panel detect: NT35510 other error attempt {}", attempt);
            }
        }
        block_for(Duration::from_millis(5));
    }

    if mismatch_count >= 2 && consistent_mismatch {
        let mut otm = Otm8009A::new();
        if otm.id_matches(&mut raw_dsi).unwrap_or(false) {
            info!("panel detect: OTM8009A fallback");
            return Panel::Otm8009a;
        }
    }

    info!("panel detect: defaulting to NT35510");
    Panel::Nt35510
}

struct BusyDelay;

impl embedded_hal::delay::DelayNs for BusyDelay {
    fn delay_ns(&mut self, ns: u32) {
        let us = (ns.saturating_add(999)) / 1000;
        if us > 0 {
            block_for(Duration::from_micros(us as u64));
        }
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_stm32::Config::default();
    config.rcc.sys = Sysclk::PLL1_P;
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV4;
    config.rcc.apb2_pre = APBPrescaler::DIV2;
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
        divq: None,
        divr: Some(PllRDiv::DIV7),
    });

    let mut p = embassy_stm32::init(config);
    info!("embassy_display_bsp_minimal: start");

    let sdram = SdramCtrl::new(&mut p, 168_000_000);
    let mut led_green = Output::new(p.PG6, Level::High, Speed::Low);
    let mut led_orange = Output::new(p.PD4, Level::High, Speed::Low);
    let mut led_red = Output::new(p.PD5, Level::High, Speed::Low);
    let mut led_blue = Output::new(p.PK3, Level::High, Speed::Low);
    let mut reset = Output::new(p.PH7, Level::Low, Speed::High);

    // LED phase signaling: blinks green LED N times to show which init phase we reached.
    // If the MCU hangs or crashes, the last blink count tells us where it died.
    fn led_phase(green: &mut Output<'_>, phase: u32) {
        // Turn off orange to signal we're in init sequence
        for _ in 0..phase {
            green.set_low();
            block_for(Duration::from_millis(80));
            green.set_high();
            block_for(Duration::from_millis(80));
        }
        // Long off pause so you can count the blinks
        green.set_low();
        block_for(Duration::from_millis(500));
    }

    led_phase(&mut led_green, 1); // Phase 1: embassy init done, peripherals claimed

    block_for(Duration::from_millis(20));
    reset.set_high();
    block_for(Duration::from_millis(140));

    let fb: &'static mut [u16] = sdram.subslice_mut(0, FB_PIXELS);
    info!("framebuffer: ptr=0x{:08x} len={}", fb.as_ptr() as u32, fb.len());
    fill_test_pattern(fb);

    let mut ltdc = Ltdc::new(p.LTDC);
    let mut dsi = DsiHost::new(p.DSIHOST, p.PJ2);

    dsi.disable_wrapper_dsi();
    dsi.disable();

    unsafe {
        reg32_write(DSI_BASE, DSI_PCTLR, 0);
        reg32_clear(DSI_BASE, DSI_WRPCR, 1 << 0);
        reg32_clear(DSI_BASE, DSI_WRPCR, 1 << 24);
        reg32_set(DSI_BASE, DSI_WRPCR, 1 << 24);
    }

    for _ in 0..1000 {
        if unsafe { reg32(DSI_BASE, DSI_WISR) & (1 << 12) != 0 } {
            break;
        }
        block_for(Duration::from_millis(1));
    }
    info!("after regulator enable");
    led_phase(&mut led_green, 2);
    log_core_state("regulator");

    unsafe {
        reg32_modify(DSI_BASE, DSI_WRPCR, |w| {
            (w & !(0x7f << 2 | 0x0f << 11 | 0x03 << 16)) | (125 << 2) | (0x02 << 11)
        });
        reg32_set(DSI_BASE, DSI_WRPCR, 1 << 0);
    }

    for _ in 0..1000 {
        if unsafe { reg32(DSI_BASE, DSI_WISR) & (1 << 8) != 0 } {
            break;
        }
        block_for(Duration::from_millis(1));
    }
    info!("after pll enable");
    led_phase(&mut led_green, 3);
    log_core_state("pll");

    unsafe {
        reg32_write(DSI_BASE, DSI_PCTLR, 0b11);
        reg32_modify(DSI_BASE, DSI_CLCR, |w| w | 1);
        reg32_modify(DSI_BASE, DSI_PCONFR, |w| (w & !0x03) | 0x01 | (10 << 8));
        reg32_write(DSI_BASE, DSI_CCR, 4);
        // UIX4 = 4_000_000_000 / f_phy_hz where f_phy = (125*8M)/2/1 = 500MHz → UIX4 = 8
        reg32_write(DSI_BASE, DSI_WPCR0, 8);
        reg32_write(DSI_BASE, DSI_IER0, 0);
        reg32_write(DSI_BASE, DSI_IER1, 0);
        reg32_write(DSI_BASE, DSI_PCR, 1 << 2); // BTAE

        // Video mode (not command mode)
        reg32_clear(DSI_BASE, DSI_MCR, 1 << 0);
        reg32_clear(DSI_BASE, DSI_WCFGR, 1 << 0);
        reg32_write(
            DSI_BASE,
            DSI_VMCR,
            0x02 // VMT = burst
                | (1 << 8)  // LPVSAE
                | (1 << 9)  // LPVBPE
                | (1 << 10) // LPVFPE
                | (1 << 11) // LPVAE
                | (1 << 12) // LPHBPE
                | (1 << 13) // LPHFPE
                | (1 << 15), // LPCE
        );
        reg32_write(DSI_BASE, DSI_VPCR, LCD_X_SIZE as u32);
        reg32_write(DSI_BASE, DSI_VCCR, 1); // NUMC=1 (1 chunk per line, matching sync HAL)
        reg32_write(DSI_BASE, DSI_VNPCR, 0x0fff);
        reg32_write(DSI_BASE, DSI_LVCIDR, 0);
        reg32_write(DSI_BASE, DSI_LPCR, 0);
        reg32_write(DSI_BASE, DSI_LCOLCR, 0x00);
        reg32_modify(DSI_BASE, DSI_WCFGR, |w| w & !(0x07 << 1));
        reg32_write(DSI_BASE, DSI_VHSACR, 4);
        reg32_write(DSI_BASE, DSI_VHBPCR, 77);
        reg32_write(DSI_BASE, DSI_VLCR, 1252);
        reg32_write(DSI_BASE, DSI_VVSACR, 1);
        reg32_write(DSI_BASE, DSI_VVBPCR, 15);
        reg32_write(DSI_BASE, DSI_VVFPCR, 16);
        reg32_write(DSI_BASE, DSI_VVACR, LCD_Y_SIZE as u32);
        reg32_write(DSI_BASE, DSI_LPMCR, 64 | (64 << 16)); // LPSIZE=64, VLPSIZE=64 (matching sync)
        reg32_write(DSI_BASE, DSI_CLTCR, (35 << 16) | 35);
        reg32_write(DSI_BASE, DSI_DLTCR, (35 << 24) | (35 << 16));
        // CMCR: AllInLowPower for panel init commands (matching sync sequence)
        reg32_write(DSI_BASE, DSI_CMCR, (1 << 24) | (0xF << 16) | (0x7F << 8));
    }
    info!("after dsi video config");
    led_phase(&mut led_green, 4);
    log_core_state("dsi-config");

    dsi.enable();
    info!("after dsi.enable");
    led_phase(&mut led_green, 5);
    dsi.enable_wrapper_dsi();
    info!("after dsi.enable_wrapper_dsi");
    led_phase(&mut led_green, 6);
    block_for(Duration::from_millis(20));
    info!("after dsi enable + 20ms delay");

    let panel = detect_panel();
    info!("after panel detect");
    led_phase(&mut led_green, 7);

    ltdc.disable();
    unsafe {
        reg32_write(RCC_BASE, RCC_PLLSAICFGR, (384 << 6) | (7 << 28));
        reg32_modify(RCC_BASE, RCC_DCKCFGR, |w| (w & !(0x3 << 16)) | (0x0 << 16));
        reg32_set(RCC_BASE, RCC_CR, 1 << 28);
        while reg32(RCC_BASE, RCC_CR) & (1 << 29) == 0 {}

        reg32_write(LTDC_BASE, LTDC_SSCR, (0 << 16) | 1);
        reg32_write(LTDC_BASE, LTDC_BPCR, (15 << 16) | 35);
        reg32_write(LTDC_BASE, LTDC_AWCR, (815 << 16) | 515);
        reg32_write(LTDC_BASE, LTDC_TWCR, (831 << 16) | 549);
        reg32_modify(LTDC_BASE, LTDC_GCR, |w| {
            (w & !((0xF << 28) | 0x3)) | (1 << 28) | (1 << 29) | (1 << 31)
        });
        reg32_write(LTDC_BASE, LTDC_BCCR, 0xAAAAAAAA);
        reg32_write(LTDC_BASE, LTDC_SRCR, 0x01);
        reg32_set(LTDC_BASE, LTDC_GCR, (1 << 0) | (1 << 1));
        reg32_write(LTDC_BASE, LTDC_SRCR, 0x01);
    }
    info!("after ltdc config (DEN+LTDCEN set)");
    led_phase(&mut led_green, 8);

    // Match sync BSP: force RX low power + AllInLowPower before panel init
    unsafe {
        reg32_set(DSI_BASE, DSI_WPCR1, 1 << 0); // FLPRXLPM
    }

    match panel {
        Panel::Nt35510 => {
            led_blue.set_high();
            led_red.set_high();
            write_nt35510_init();
        }
        Panel::Otm8009a => {
            led_blue.set_low();
            led_red.set_high();
            write_otm8009a_init();
        }
    }
    led_phase(&mut led_green, 9);
    info!("after panel init");

    // Match sync BSP: disable force RX low power + switch to AllInHighSpeed
    unsafe {
        reg32_clear(DSI_BASE, DSI_WPCR1, 1 << 0);
        reg32_write(DSI_BASE, DSI_CMCR, 0); // AllInHighSpeed
    }

    unsafe {
        reg32_write(LTDC_BASE, LTDC_L1_BASE + 0x04, ((515u32) << 16) | 36);
        reg32_write(LTDC_BASE, LTDC_L1_BASE + 0x08, ((815u32) << 16) | 16);
        reg32_write(LTDC_BASE, LTDC_L1_BASE + 0x10, 0x02);
        reg32_write(LTDC_BASE, LTDC_L1_BASE + 0x18, 0x0000_0000);
        reg32_write(LTDC_BASE, LTDC_L1_BASE + 0x14, 0x0000_00FF);
        reg32_write(LTDC_BASE, LTDC_L1_BASE + 0x1C, (7 << 8) | 4);
        reg32_write(LTDC_BASE, LTDC_L1_BASE + 0x28, fb.as_ptr() as u32);
        reg32_write(LTDC_BASE, LTDC_L1_BASE + 0x2C, ((LCD_X_SIZE as u32 * 2 + 3) << 16) | (LCD_X_SIZE as u32 * 2));
        reg32_write(LTDC_BASE, LTDC_L1_BASE + 0x30, LCD_Y_SIZE as u32);
        reg32_set(LTDC_BASE, LTDC_L1_BASE, 1);
        reg32_write(LTDC_BASE, LTDC_SRCR, 0x01);

        reg32_set(LTDC_BASE, LTDC_GCR, (1 << 0) | (1 << 1));
        reg32_write(LTDC_BASE, LTDC_SRCR, 0x01);
        reg32_set(DSI_BASE, DSI_WCR, 1 << 2);
    }

    log_core_state("final");
    info!(
        "WPCR1={=u32:08x} CMCR={=u32:08x} BCCR={=u32:08x} L1CFBAR={=u32:08x} L1CFBLR={=u32:08x} L1CFBLNR={=u32:08x}",
        unsafe { reg32(DSI_BASE, DSI_WPCR1) },
        unsafe { reg32(DSI_BASE, DSI_CMCR) },
        unsafe { reg32(LTDC_BASE, LTDC_BCCR) },
        unsafe { reg32(LTDC_BASE, LTDC_L1_BASE + 0x28) },
        unsafe { reg32(LTDC_BASE, LTDC_L1_BASE + 0x2C) },
        unsafe { reg32(LTDC_BASE, LTDC_L1_BASE + 0x30) },
    );

    led_phase(&mut led_green, 10);
    info!("embassy_display_bsp_minimal: init done");
    loop {
        led_orange.set_high();
        Timer::after_millis(500).await;
        led_orange.set_low();
        Timer::after_millis(500).await;
    }
}

unsafe fn raw_dsi_write_cmd(address: u8, data: &[u8]) {
    let dt: u8 = if data.len() <= 1 { 0x15 } else { 0x39 };

    if data.len() <= 1 {
        let param = if data.len() == 1 { data[0] } else { 0 };
        reg32_write(DSI_BASE, DSI_GHCR, (dt as u32) | ((param as u32) << 8) | ((address as u32) << 16));
    } else {
        let mut word = address as u32;
        for (i, &b) in data.iter().take(3).enumerate() {
            word |= (b as u32) << (8 + 8 * i);
        }
        reg32_write(DSI_BASE, DSI_GPDR, word);

        if data.len() > 3 {
            let mut iter = data[3..].chunks_exact(4);
            for chunk in &mut iter {
                let w = u32::from_ne_bytes(chunk.try_into().unwrap());
                reg32_write(DSI_BASE, DSI_GPDR, w);
            }
            if !iter.remainder().is_empty() {
                let mut w = 0u32;
                for (i, &b) in iter.remainder().iter().enumerate() {
                    w |= (b as u32) << (8 * i);
                }
                reg32_write(DSI_BASE, DSI_GPDR, w);
            }
        }

        let len = (data.len() + 1) as u32;
        reg32_write(
            DSI_BASE,
            DSI_GHCR,
            (dt as u32) | ((len & 0xFF) as u32) << 8 | (((len >> 8) & 0xFF) as u32) << 16,
        );
    }
}

fn fill_test_pattern(fb: &mut [u16]) {
    for y in 0..LCD_Y_SIZE as usize {
        for x in 0..LCD_X_SIZE as usize {
            fb[y * LCD_X_SIZE as usize + x] = if y < 120 {
                0xF800
            } else if y < 240 {
                0x07E0
            } else if y < 360 {
                0x001F
            } else if ((x / 16) + (y / 16)) % 2 == 0 {
                0xFFFF
            } else {
                0x0000
            };
        }
    }
}

fn write_nt35510_init() {
    info!("panel init: start");

    // Page 1 commands (SETETC page=1, B0-BE registers)
    for cmd in NT35510_PAGE1 {
        unsafe { raw_dsi_write_cmd(cmd[0], &cmd[1..]) };
    }
    info!("panel init: page-1 done");

    // Page 0 commands (SETETC page=0, B1-CC-BA registers)
    for cmd in NT35510_PAGE0 {
        unsafe { raw_dsi_write_cmd(cmd[0], &cmd[1..]) };
    }
    info!("panel init: page-0 done");

    // Pre-sleep: TEEON + COLMOD RGB888 (before SLPOUT, matching nt35510 crate)
    for cmd in NT35510_PRE_SLEEP {
        unsafe { raw_dsi_write_cmd(cmd[0], &cmd[1..]) };
    }

    // SLPOUT with delays (matching nt35510 crate: 200ms pre, SLPOUT, 120ms post)
    block_for(Duration::from_millis(200));
    unsafe { raw_dsi_write_cmd(0x11, &[0x00]) }; // SLPOUT
    block_for(Duration::from_millis(120));

    // Post-sleep: MADCTL, CASET, RASET, COLMOD RGB565, brightness, backlight
    for cmd in NT35510_POST_SLEEP_PRE_DISPLAY {
        unsafe { raw_dsi_write_cmd(cmd[0], &cmd[1..]) };
    }

    // 10ms delay before DISPON (matching nt35510 crate)
    block_for(Duration::from_millis(10));

    // DISPON + 10ms delay + RAMWR
    for cmd in NT35510_POST_SLEEP_DISPLAY {
        unsafe { raw_dsi_write_cmd(cmd[0], &cmd[1..]) };
    }
    block_for(Duration::from_millis(10));

    info!("panel init: done");
}

fn write_otm8009a_init() {
    info!("panel init: OTM8009A start");
    let mut raw_dsi = RawDsi;
    let mut panel = Otm8009A::new();
    let mut delay = BusyDelay;
    let config = Otm8009AConfig {
        frame_rate: OtmFrameRate::_60Hz,
        mode: OtmMode::Portrait,
        color_map: OtmColorMap::Rgb,
        cols: LCD_X_SIZE,
        rows: LCD_Y_SIZE,
    };
    panel.init(&mut raw_dsi, config, &mut delay).unwrap();
    info!("panel init: OTM8009A done");
}

const NT35510_PAGE1: &[&[u8]] = &[
    &[0xF0, 0x55, 0xAA, 0x52, 0x08, 0x01],
    &[0xB0, 0x03, 0x03, 0x03],
    &[0xB6, 0x46, 0x46, 0x46],
    &[0xB1, 0x03, 0x03, 0x03],
    &[0xB7, 0x36, 0x36, 0x36],
    &[0xB2, 0x00, 0x00, 0x02],
    &[0xB8, 0x26, 0x26, 0x26],
    &[0xBF, 0x01],
    &[0xB3, 0x09, 0x09, 0x09],
    &[0xB9, 0x36, 0x36, 0x36],
    &[0xB5, 0x08, 0x08, 0x08],
    &[0xBA, 0x26, 0x26, 0x26],
    &[0xBC, 0x00, 0x80, 0x00],
    &[0xBD, 0x00, 0x80, 0x00],
    &[0xBE, 0x00, 0x50],
];

const NT35510_PAGE0: &[&[u8]] = &[
    &[0xF0, 0x55, 0xAA, 0x52, 0x08, 0x00],
    &[0xB1, 0xFC, 0x00],
    &[0xB6, 0x03, 0x03],
    &[0xB5, 0x50, 0x50],
    &[0xB7, 0x00, 0x00],
    &[0xB8, 0x01, 0x02, 0x02, 0x02],
    &[0xBC, 0x00, 0x00, 0x00],
    &[0xCC, 0x03, 0x00, 0x00],
    &[0xBA, 0x01, 0x01],
];

const NT35510_PRE_SLEEP: &[&[u8]] = &[
    &[0x35, 0x00], // TEEON VBLANKING_INFO_ONLY
    &[0x3A, 0x77], // COLMOD RGB888
];

const NT35510_POST_SLEEP_PRE_DISPLAY: &[&[u8]] = &[
    &[0x36, 0x00],                     // MADCTL Portrait
    &[0x2A, 0x00, 0x00, 0x01, 0xDF], // CASET
    &[0x2B, 0x00, 0x00, 0x03, 0x1F], // RASET
    &[0x3A, 0x55],                     // COLMOD RGB565
    &[0x51, 0x7F],                     // WRDISBV
    &[0x53, 0x2C],                     // WRCTRLD BL_ON
    &[0x55, 0x02],                     // WRCABC
    &[0x5E, 0xFF],                     // WRCABCMB
];

const NT35510_POST_SLEEP_DISPLAY: &[&[u8]] = &[
    &[0x29, 0x00], // DISPON
    &[0x2C, 0x00], // RAMWR
];
