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
use embassy_time::{Duration, Timer, block_for};
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
    led_phase(&mut led_green, 5);

    dsi.enable();
    info!("after dsi.enable");
    led_phase(&mut led_green, 6);
    dsi.enable_wrapper_dsi();
    info!("after dsi.enable_wrapper_dsi");
    led_phase(&mut led_green, 7);
    block_for(Duration::from_millis(120));
    info!("after dsi enable + 120ms delay");

    // Match sync BSP: force RX low power + AllInLowPower before panel init
    unsafe {
        reg32_set(DSI_BASE, DSI_WPCR1, 1 << 0); // FLPRXLPM
    }

    write_nt35510_init();
    led_phase(&mut led_green, 8);
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

    led_phase(&mut led_green, 9);
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
