//! Minimal DSI display test for STM32F469I-DISCO (portrait 480x800)
//!
//! Mirrors the verified-working embassy dsi_bsp.rs example (commit 83e0d37)
//! with portrait adaptation and SDRAM framebuffer.
//!
//! Build:
//!   cargo build --release --target thumbv7em-none-eabihf \
//!     --manifest-path examples/stm32f469i-disco/Cargo.toml \
//!     --bin display_minimal --no-default-features --features scanner-async,defmt
//!
//! Flash (RTT debug — USB will NOT work):
//!   arm-none-eabi-objcopy -O binary target/thumbv7em-none-eabihf/release/display_minimal /tmp/display_minimal.bin
//!   st-flash --connect-under-reset write /tmp/display_minimal.bin 0x08000000
//!   st-flash --connect-under-reset reset

#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::dsihost::{DsiHost, PacketType};
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::ltdc::Ltdc;
use embassy_stm32::rcc::{
    mux, AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllRDiv, PllSource, Sysclk,
};
use embassy_stm32::time::mhz;
use embassy_stm32f469i_disco::display::SdramCtrl;
use embassy_time::{Duration, Timer, block_for};
use linked_list_allocator::LockedHeap;
use stm32_metapac::dsihost::regs::{Ier0, Ier1};
use stm32_metapac::ltdc::vals::{Bf1, Bf2, Depol, Hspol, Imr, Pcpol, Pf, Vspol};
use stm32_metapac::{DSIHOST, LTDC};
use {defmt_rtt as _, panic_probe as _};

const HEAP_SIZE: usize = 1024;
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();
#[allow(dead_code)]
static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

// Portrait mode: 480 wide × 800 tall
const LCD_X_SIZE: u16 = 480;
const LCD_Y_SIZE: u16 = 800;

// ── NT35510 DCS commands (from verified working embassy dsi_bsp.rs) ──

const NT35510_CMD_TEEON: u8 = 0x35;
const NT35510_CMD_MADCTL: u8 = 0x36;
const NT35510_CMD_SLPOUT: u8 = 0x11;
const NT35510_CMD_DISPON: u8 = 0x29;
const NT35510_CMD_CASET: u8 = 0x2A;
const NT35510_CMD_RASET: u8 = 0x2B;
const NT35510_CMD_RAMWR: u8 = 0x2C;
const NT35510_CMD_COLMOD: u8 = 0x3A;
const NT35510_CMD_WRDISBV: u8 = 0x51;
const NT35510_CMD_WRCTRLD: u8 = 0x53;
const NT35510_CMD_WRCABC: u8 = 0x55;
const NT35510_CMD_WRCABCMB: u8 = 0x5E;
const NT35510_COLMOD_RGB888: u8 = 0x77;

// Page 1 commands (power/voltage init)
const NT35510_WRITES_0: &[u8] = &[0xF0, 0x55, 0xAA, 0x52, 0x08, 0x01]; // Page 1 enable
const NT35510_WRITES_1: &[u8] = &[0xB0, 0x03, 0x03, 0x03]; // AVDD: 5.2V
const NT35510_WRITES_2: &[u8] = &[0xB6, 0x46, 0x46, 0x46]; // AVDD: Ratio
const NT35510_WRITES_3: &[u8] = &[0xB1, 0x03, 0x03, 0x03]; // AVEE: -5.2V
const NT35510_WRITES_4: &[u8] = &[0xB7, 0x36, 0x36, 0x36]; // AVEE: Ratio
const NT35510_WRITES_5: &[u8] = &[0xB2, 0x00, 0x00, 0x02]; // VCL: -2.5V
const NT35510_WRITES_6: &[u8] = &[0xB8, 0x26, 0x26, 0x26]; // VCL: Ratio
const NT35510_WRITES_7: &[u8] = &[0xBF, 0x01]; // VGH: 15V
const NT35510_WRITES_8: &[u8] = &[0xB3, 0x09, 0x09, 0x09];
const NT35510_WRITES_9: &[u8] = &[0xB9, 0x36, 0x36, 0x36]; // VGH: Ratio
const NT35510_WRITES_10: &[u8] = &[0xB5, 0x08, 0x08, 0x08]; // VGL_REG: -10V
const NT35510_WRITES_12: &[u8] = &[0xBA, 0x26, 0x26, 0x26]; // VGLX: Ratio
const NT35510_WRITES_13: &[u8] = &[0xBC, 0x00, 0x80, 0x00]; // VGMP/VGSP: 4.5V/0V
const NT35510_WRITES_14: &[u8] = &[0xBD, 0x00, 0x80, 0x00]; // VGMN/VGSN: -4.5V/0V
const NT35510_WRITES_15: &[u8] = &[0xBE, 0x00, 0x50]; // VCOM: -1.325V
const NT35510_WRITES_16: &[u8] = &[0xF0, 0x55, 0xAA, 0x52, 0x08, 0x00]; // Page 0 enable
const NT35510_WRITES_17: &[u8] = &[0xB1, 0xFC, 0x00]; // Display control
const NT35510_WRITES_18: &[u8] = &[0xB6, 0x03]; // Src hold time
const NT35510_WRITES_19: &[u8] = &[0xB5, 0x51];
const NT35510_WRITES_20: &[u8] = &[0x00, 0x00, 0xB7]; // Gate EQ control
const NT35510_WRITES_21: &[u8] = &[0xB8, 0x01, 0x02, 0x02, 0x02]; // Src EQ control
const NT35510_WRITES_22: &[u8] = &[0xBC, 0x00, 0x00, 0x00]; // Inv. mode
const NT35510_WRITES_23: &[u8] = &[0xCC, 0x03, 0x00, 0x00];
const NT35510_WRITES_24: &[u8] = &[0xBA, 0x01];
const NT35510_WRITES_26: &[u8] = &[NT35510_CMD_TEEON, 0x00]; // Tear on
const NT35510_WRITES_27: &[u8] = &[NT35510_CMD_SLPOUT, 0x00]; // Sleep out
const NT35510_WRITES_30: &[u8] = &[NT35510_CMD_DISPON, 0x00]; // Display on
const NT35510_WRITES_31: &[u8] = &[NT35510_CMD_WRDISBV, 0x7F];
const NT35510_WRITES_32: &[u8] = &[NT35510_CMD_WRCTRLD, 0x2C];
const NT35510_WRITES_33: &[u8] = &[NT35510_CMD_WRCABC, 0x02];
const NT35510_WRITES_34: &[u8] = &[NT35510_CMD_WRCABCMB, 0xFF];
const NT35510_WRITES_35: &[u8] = &[NT35510_CMD_RAMWR, 0x00];
const NT35510_WRITES_37: &[u8] = &[NT35510_CMD_COLMOD, NT35510_COLMOD_RGB888];

// Portrait orientation commands
const NT35510_MADCTL_PORTRAIT: &[u8] = &[NT35510_CMD_MADCTL, 0x00];
const NT35510_CASET_PORTRAIT: &[u8] = &[NT35510_CMD_CASET, 0x00, 0x00, 0x01, 0xDF]; // 0..479
const NT35510_RASET_PORTRAIT: &[u8] = &[NT35510_CMD_RASET, 0x00, 0x00, 0x03, 0x1F]; // 0..799

// ── Main ──

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
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
        divq: Some(PllQDiv::DIV8), // 48MHz for USB/RNG (CK48SEL=PLLSAI1_Q)
        divr: Some(PllRDiv::DIV7),
    });

    config.rcc.mux.clk48sel = mux::Clk48sel::PLLSAI1_Q;

    let mut p = embassy_stm32::init(config);
    info!("display_minimal: starting (portrait 480x800)");

    // ── SDRAM init (must be before moving peripherals out of p) ──
    let sdram = SdramCtrl::new(&mut p, 180_000_000);
    info!("display_minimal: SDRAM initialized");

    // ── GPIO ──
    let mut led = Output::new(p.PG6, Level::High, Speed::Low);

    // PH7 = active-low reset for LCD and touchsensor
    let mut reset = Output::new(p.PH7, Level::Low, Speed::High);
    block_for(Duration::from_millis(20));
    reset.set_high();
    block_for(Duration::from_millis(140));
    core::mem::forget(reset);

    // ── Create DSI/LTDC peripherals ──
    let mut ltdc = Ltdc::new(p.LTDC);
    let mut dsi = DsiHost::new(p.DSIHOST, p.PJ2);
    let version = dsi.get_version();
    info!("display_minimal: DSI version={:x}", version);

    // ── DSI init: identical to verified working dsi_bsp.rs ──

    // Disable the DSI wrapper and host
    dsi.disable_wrapper_dsi();
    dsi.disable();

    // D-PHY clock and digital disable
    DSIHOST.pctlr().modify(|w| {
        w.set_cke(false);
        w.set_den(false)
    });

    // Turn off the DSI PLL
    DSIHOST.wrpcr().modify(|w| w.set_pllen(false));

    // Disable the regulator
    DSIHOST.wrpcr().write(|w| w.set_regen(false));

    // Enable regulator
    info!("DSIHOST: enabling regulator");
    DSIHOST.wrpcr().write(|w| w.set_regen(true));

    for _ in 1..1000 {
        if DSIHOST.wisr().read().rrs() {
            info!("DSIHOST Regulator ready");
            break;
        }
        block_for(Duration::from_millis(1));
    }
    if !DSIHOST.wisr().read().rrs() {
        defmt::panic!("DSIHOST: regulator FAILED");
    }

    // Set up PLL and enable it
    DSIHOST.wrpcr().modify(|w| {
        w.set_pllen(true);
        w.set_ndiv(125);
        w.set_idf(2);
        w.set_odf(0);
    });

    const LANE_BYTE_CLK_K_HZ: u16 = 62500;
    const _LCD_CLOCK: u16 = 27429;
    const TX_ESCAPE_CKDIV: u8 = (LANE_BYTE_CLK_K_HZ / 15620) as u8;

    for _ in 1..1000 {
        block_for(Duration::from_millis(1));
        if DSIHOST.wisr().read().pllls() {
            info!("DSIHOST PLL locked");
            break;
        }
    }
    if !DSIHOST.wisr().read().pllls() {
        defmt::panic!("DSIHOST: PLL FAILED");
    }

    // D-PHY clock and digital enable
    DSIHOST.pctlr().write(|w| {
        w.set_cke(true);
        w.set_den(true);
    });

    // Clock lane to high-speed mode, disable automatic clock lane control
    DSIHOST.clcr().modify(|w| {
        w.set_dpcc(true);
        w.set_acr(false);
    });

    // Two active data lanes
    DSIHOST.pconfr().modify(|w| w.set_nl(1));

    // TX escape clock division
    DSIHOST.ccr().modify(|w| w.set_txeckdiv(TX_ESCAPE_CKDIV));

    // UIX4 = 8 (bit period in 0.25ns units)
    DSIHOST.wpcr0().modify(|w| w.set_uix4(8));

    // Disable error interrupts
    DSIHOST.ier0().write_value(Ier0(0));
    DSIHOST.ier1().write_value(Ier1(0));

    // Enable BTA to fix read timeout
    DSIHOST.pcr().modify(|w| w.set_btae(true));

    // ── Video mode config ──

    const DSI_PIXEL_FORMAT_RGB888: u8 = 0x05;

    const HACT: u16 = LCD_X_SIZE; // 480 (portrait)
    const VACT: u16 = LCD_Y_SIZE; // 800 (portrait)

    const VSA: u16 = 120;
    const VBP: u16 = 150;
    const VFP: u16 = 150;
    const HSA: u16 = 2;
    const HBP: u16 = 34;
    const HFP: u16 = 34;

    const COLOR_CODING: u8 = DSI_PIXEL_FORMAT_RGB888;
    const VS_POLARITY: bool = false;
    const HS_POLARITY: bool = false;
    const DE_POLARITY: bool = false;
    const MODE: u8 = 2; // Video burst
    const NULL_PACKET_SIZE: u16 = 0xFFF;
    const NUMBER_OF_CHUNKS: u16 = 0;
    const PACKET_SIZE: u16 = HACT; // 480

    // DSI lane byte clock cycle values (from ST BSP, verified working)
    const HORIZONTAL_SYNC_ACTIVE: u16 = 4;
    const HORIZONTAL_BACK_PORCH: u16 = 77;
    // Portrait: (480+2+34+34)*62500/27429 = 1253
    const HORIZONTAL_LINE: u16 = 1253;

    const VERTICAL_SYNC_ACTIVE: u16 = VSA;
    const VERTICAL_BACK_PORCH: u16 = VBP;
    const VERTICAL_FRONT_PORCH: u16 = VFP;
    const VERTICAL_ACTIVE: u16 = VACT;
    const LP_COMMAND_ENABLE: bool = true;
    const LP_LARGEST_PACKET_SIZE: u8 = 16;
    const LPVACT_LARGEST_PACKET_SIZE: u8 = 0;

    const LPHORIZONTAL_FRONT_PORCH_ENABLE: bool = true;
    const LPHORIZONTAL_BACK_PORCH_ENABLE: bool = true;
    const LPVERTICAL_ACTIVE_ENABLE: bool = true;
    const LPVERTICAL_FRONT_PORCH_ENABLE: bool = true;
    const LPVERTICAL_BACK_PORCH_ENABLE: bool = true;
    const LPVERTICAL_SYNC_ACTIVE_ENABLE: bool = true;
    const FRAME_BTAACKNOWLEDGE_ENABLE: bool = false;

    // Select video mode
    DSIHOST.mcr().modify(|w| w.set_cmdm(false));
    DSIHOST.wcfgr().modify(|w| w.set_dsim(false));

    DSIHOST.vmcr().modify(|w| w.set_vmt(MODE));
    DSIHOST.vpcr().modify(|w| w.set_vpsize(PACKET_SIZE));
    DSIHOST.vccr().modify(|w| w.set_numc(NUMBER_OF_CHUNKS));
    DSIHOST.vnpcr().modify(|w| w.set_npsize(NULL_PACKET_SIZE));
    DSIHOST.lvcidr().modify(|w| w.set_vcid(0));

    DSIHOST.lpcr().modify(|w| {
        w.set_dep(DE_POLARITY);
        w.set_hsp(HS_POLARITY);
        w.set_vsp(VS_POLARITY);
    });

    DSIHOST.lcolcr().modify(|w| w.set_colc(COLOR_CODING));
    DSIHOST.wcfgr().modify(|w| w.set_colmux(COLOR_CODING));

    // DSI timing (lane byte clock cycles)
    DSIHOST.vhsacr().modify(|w| w.set_hsa(HORIZONTAL_SYNC_ACTIVE));
    DSIHOST.vhbpcr().modify(|w| w.set_hbp(HORIZONTAL_BACK_PORCH));
    DSIHOST.vlcr().modify(|w| w.set_hline(HORIZONTAL_LINE));
    DSIHOST.vvsacr().modify(|w| w.set_vsa(VERTICAL_SYNC_ACTIVE));
    DSIHOST.vvbpcr().modify(|w| w.set_vbp(VERTICAL_BACK_PORCH));
    DSIHOST.vvfpcr().modify(|w| w.set_vfp(VERTICAL_FRONT_PORCH));
    DSIHOST.vvacr().modify(|w| w.set_va(VERTICAL_ACTIVE));

    DSIHOST.vmcr().modify(|w| w.set_lpce(LP_COMMAND_ENABLE));

    DSIHOST.lpmcr().modify(|w| w.set_lpsize(LP_LARGEST_PACKET_SIZE));
    DSIHOST.lpmcr().modify(|w| w.set_lpsize(LP_LARGEST_PACKET_SIZE));
    DSIHOST.lpmcr().modify(|w| w.set_vlpsize(LPVACT_LARGEST_PACKET_SIZE));

    DSIHOST.vmcr().modify(|w| w.set_lphfpe(LPHORIZONTAL_FRONT_PORCH_ENABLE));
    DSIHOST.vmcr().modify(|w| w.set_lphbpe(LPHORIZONTAL_BACK_PORCH_ENABLE));
    DSIHOST.vmcr().modify(|w| w.set_lpvae(LPVERTICAL_ACTIVE_ENABLE));
    DSIHOST.vmcr().modify(|w| w.set_lpvfpe(LPVERTICAL_FRONT_PORCH_ENABLE));
    DSIHOST.vmcr().modify(|w| w.set_lpvbpe(LPVERTICAL_BACK_PORCH_ENABLE));
    DSIHOST.vmcr().modify(|w| w.set_lpvsae(LPVERTICAL_SYNC_ACTIVE_ENABLE));
    DSIHOST.vmcr().modify(|w| w.set_fbtaae(FRAME_BTAACKNOWLEDGE_ENABLE));

    // PHY HS2LP and LP2HS timings
    const CLOCK_LANE_HS2_LPTIME: u16 = 35;
    const CLOCK_LANE_LP2_HSTIME: u16 = 35;
    const DATA_LANE_HS2_LPTIME: u8 = 35;
    const DATA_LANE_LP2_HSTIME: u8 = 35;
    const DATA_LANE_MAX_READ_TIME: u16 = 0;
    const STOP_WAIT_TIME: u8 = 10;

    const MAX_TIME: u16 = if CLOCK_LANE_HS2_LPTIME > CLOCK_LANE_LP2_HSTIME {
        CLOCK_LANE_HS2_LPTIME
    } else {
        CLOCK_LANE_LP2_HSTIME
    };

    DSIHOST.cltcr().modify(|w| {
        w.set_hs2lp_time(MAX_TIME);
        w.set_lp2hs_time(MAX_TIME)
    });

    DSIHOST.dltcr().modify(|w| {
        w.set_hs2lp_time(DATA_LANE_HS2_LPTIME);
        w.set_lp2hs_time(DATA_LANE_LP2_HSTIME);
        w.set_mrd_time(DATA_LANE_MAX_READ_TIME);
    });

    DSIHOST.pconfr().modify(|w| w.set_sw_time(STOP_WAIT_TIME));

    // ── LTDC init (portrait timing) ──

    const LTDC_DE_POLARITY: Depol = Depol::ACTIVE_LOW;
    const LTDC_VS_POLARITY: Vspol = Vspol::ACTIVE_HIGH;
    const LTDC_HS_POLARITY: Hspol = Hspol::ACTIVE_HIGH;

    // Portrait accumulated values
    const HORIZONTAL_SYNC: u16 = HSA - 1; // 1
    const VERTICAL_SYNC: u16 = VERTICAL_SYNC_ACTIVE - 1; // 119
    const ACCUMULATED_HBP: u16 = HSA + HBP - 1; // 35
    const ACCUMULATED_VBP: u16 = VERTICAL_SYNC_ACTIVE + VERTICAL_BACK_PORCH - 1; // 269
    const ACCUMULATED_ACTIVE_W: u16 = LCD_X_SIZE + HSA + HBP - 1; // 515
    const ACCUMULATED_ACTIVE_H: u16 = VERTICAL_SYNC_ACTIVE + VERTICAL_BACK_PORCH + VERTICAL_ACTIVE - 1; // 1069
    const TOTAL_WIDTH: u16 = LCD_X_SIZE + HSA + HBP + HFP - 1; // 549
    const TOTAL_HEIGHT: u16 = VERTICAL_SYNC_ACTIVE + VERTICAL_BACK_PORCH + VERTICAL_ACTIVE + VERTICAL_FRONT_PORCH - 1; // 1219

    ltdc.disable();

    LTDC.gcr().modify(|w| {
        w.set_hspol(LTDC_HS_POLARITY);
        w.set_vspol(LTDC_VS_POLARITY);
        w.set_depol(LTDC_DE_POLARITY);
        w.set_pcpol(Pcpol::RISING_EDGE);
    });

    LTDC.sscr().modify(|w| {
        w.set_hsw(HORIZONTAL_SYNC);
        w.set_vsh(VERTICAL_SYNC)
    });

    LTDC.bpcr().modify(|w| {
        w.set_ahbp(ACCUMULATED_HBP);
        w.set_avbp(ACCUMULATED_VBP);
    });

    LTDC.awcr().modify(|w| {
        w.set_aah(ACCUMULATED_ACTIVE_H);
        w.set_aaw(ACCUMULATED_ACTIVE_W);
    });

    LTDC.twcr().modify(|w| {
        w.set_totalh(TOTAL_HEIGHT);
        w.set_totalw(TOTAL_WIDTH);
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

    // ── Enable DSI ──
    dsi.enable();
    dsi.enable_wrapper_dsi();

    // Wait before sending panel commands
    block_for(Duration::from_millis(120));

    // ── NT35510 panel init (hardcoded commands from working example) ──

    dsi.write_cmd(0, NT35510_WRITES_0[0], &NT35510_WRITES_0[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_1[0], &NT35510_WRITES_1[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_2[0], &NT35510_WRITES_2[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_3[0], &NT35510_WRITES_3[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_4[0], &NT35510_WRITES_4[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_5[0], &NT35510_WRITES_5[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_6[0], &NT35510_WRITES_6[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_7[0], &NT35510_WRITES_7[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_8[0], &NT35510_WRITES_8[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_9[0], &NT35510_WRITES_9[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_10[0], &NT35510_WRITES_10[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_12[0], &NT35510_WRITES_12[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_13[0], &NT35510_WRITES_13[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_14[0], &NT35510_WRITES_14[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_15[0], &NT35510_WRITES_15[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_16[0], &NT35510_WRITES_16[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_17[0], &NT35510_WRITES_17[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_18[0], &NT35510_WRITES_18[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_19[0], &NT35510_WRITES_19[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_20[0], &NT35510_WRITES_20[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_21[0], &NT35510_WRITES_21[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_22[0], &NT35510_WRITES_22[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_23[0], &NT35510_WRITES_23[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_24[0], &NT35510_WRITES_24[1..]).unwrap();

    // Tear on
    dsi.write_cmd(0, NT35510_WRITES_26[0], &NT35510_WRITES_26[1..]).unwrap();

    // Set pixel color format to RGB888
    dsi.write_cmd(0, NT35510_WRITES_37[0], &NT35510_WRITES_37[1..]).unwrap();

    // Delay for MADCTL to take effect
    block_for(Duration::from_millis(200));

    // Portrait orientation
    dsi.write_cmd(0, NT35510_MADCTL_PORTRAIT[0], &NT35510_MADCTL_PORTRAIT[1..]).unwrap();
    dsi.write_cmd(0, NT35510_CASET_PORTRAIT[0], &NT35510_CASET_PORTRAIT[1..]).unwrap();
    dsi.write_cmd(0, NT35510_RASET_PORTRAIT[0], &NT35510_RASET_PORTRAIT[1..]).unwrap();

    // Sleep out
    dsi.write_cmd(0, NT35510_WRITES_27[0], &NT35510_WRITES_27[1..]).unwrap();
    block_for(Duration::from_millis(120));

    // Color coding again
    dsi.write_cmd(0, NT35510_WRITES_37[0], &NT35510_WRITES_37[1..]).unwrap();

    // CABC (backlight brightness)
    dsi.write_cmd(0, NT35510_WRITES_31[0], &NT35510_WRITES_31[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_32[0], &NT35510_WRITES_32[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_33[0], &NT35510_WRITES_33[1..]).unwrap();
    dsi.write_cmd(0, NT35510_WRITES_34[0], &NT35510_WRITES_34[1..]).unwrap();

    // Display on
    dsi.write_cmd(0, NT35510_WRITES_30[0], &NT35510_WRITES_30[1..]).unwrap();

    // GRAM memory write (initiate frame write in video mode)
    dsi.write_cmd(0, NT35510_WRITES_35[0], &NT35510_WRITES_35[1..]).unwrap();

    info!("display_minimal: NT35510 init done");

    // ── SDRAM framebuffer with test pattern ──

    const PIXEL_FORMAT: Pf = Pf::ARGB8888;
    const PIXEL_SIZE: u8 = 4u8;
    const IMAGE_WIDTH: u16 = LCD_X_SIZE;
    const IMAGE_HEIGHT: u16 = LCD_Y_SIZE;

    let fb: &'static mut [u32] = sdram.subslice_mut(0, LCD_X_SIZE as usize * LCD_Y_SIZE as usize);
    let fb_addr = fb.as_mut_ptr() as u32;
    info!("display_minimal: fb addr={:010x} len={}", fb_addr, fb.len());

    // Fill with 4 horizontal color bands: red, green, blue, white
    let rows_per_band = LCD_Y_SIZE as usize / 4;
    let colors: [u32; 4] = [0xFFFF0000, 0xFF00FF00, 0xFF0000FF, 0xFFFFFFFF];
    for (i, pixel) in fb.iter_mut().enumerate() {
        let row = i / LCD_X_SIZE as usize;
        let band = core::cmp::min(row / rows_per_band, 3);
        *pixel = colors[band];
    }

    // ── LTDC layer config (portrait, ARGB8888) ──

    const WINDOW_X0: u16 = 0;
    const WINDOW_X1: u16 = LCD_X_SIZE; // 480
    const WINDOW_Y0: u16 = 0;
    const WINDOW_Y1: u16 = LCD_Y_SIZE; // 800
    const ALPHA: u8 = 255;
    const ALPHA0: u8 = 0;

    // Horizontal window position
    LTDC.layer(0).whpcr().write(|w| {
        w.set_whstpos(LTDC.bpcr().read().ahbp() + 1 + WINDOW_X0);
        w.set_whsppos(LTDC.bpcr().read().ahbp() + WINDOW_X1);
    });

    // Vertical window position
    LTDC.layer(0).wvpcr().write(|w| {
        w.set_wvstpos(LTDC.bpcr().read().avbp() + 1 + WINDOW_Y0);
        w.set_wvsppos(LTDC.bpcr().read().avbp() + WINDOW_Y1);
    });

    // Pixel format
    LTDC.layer(0).pfcr().write(|w| w.set_pf(PIXEL_FORMAT));

    // Default color
    LTDC.layer(0).dccr().modify(|w| {
        w.set_dcblue(0);
        w.set_dcgreen(0);
        w.set_dcred(0);
        w.set_dcalpha(ALPHA0);
    });

    // Constant alpha
    LTDC.layer(0).cacr().write(|w| w.set_consta(ALPHA));

    // Blending factors
    LTDC.layer(0).bfcr().write(|w| {
        w.set_bf1(Bf1::CONSTANT);
        w.set_bf2(Bf2::CONSTANT);
    });

    // Framebuffer address
    info!("display_minimal: setting fb address {:010x}", fb_addr);
    LTDC.layer(0).cfbar().write(|w| w.set_cfbadd(fb_addr));

    // Framebuffer pitch
    LTDC.layer(0).cfblr().write(|w| {
        w.set_cfbp(IMAGE_WIDTH * PIXEL_SIZE as u16);
        w.set_cfbll(((WINDOW_X1 - WINDOW_X0) * PIXEL_SIZE as u16) + 3);
    });

    // Frame buffer line number
    LTDC.layer(0).cfblnr().write(|w| w.set_cfblnbr(IMAGE_HEIGHT));

    // Enable layer
    LTDC.layer(0).cr().modify(|w| w.set_len(true));

    // Reload
    LTDC.srcr().modify(|w| w.set_imr(Imr::RELOAD));

    info!("display_minimal: LTDC layer configured");

    // ── Panel autodetection: read manufacturer/driver ID ──
    block_for(Duration::from_millis(5000));

    const READ_SIZE: u16 = 1;
    let mut data = [1u8; READ_SIZE as usize];

    match dsi.read(0, PacketType::DcsShortPktRead(0xDA), READ_SIZE, &mut data) {
        Ok(()) => info!("Panel ID1 (manufacturer): {:#04x}", data[0]),
        Err(e) => warn!("Panel ID1 read failed: {:?}", e),
    }

    match dsi.read(0, PacketType::DcsShortPktRead(0xDB), READ_SIZE, &mut data) {
        Ok(()) => info!("Panel ID2 (version):    {:#04x}", data[0]),
        Err(e) => warn!("Panel ID2 read failed: {:?}", e),
    }

    match dsi.read(0, PacketType::DcsShortPktRead(0xDC), READ_SIZE, &mut data) {
        Ok(()) => info!("Panel ID3 (driver):     {:#04x}", data[0]),
        Err(e) => warn!("Panel ID3 read failed: {:?}", e),
    }

    info!("display_minimal: init complete — blinking LED");

    // ── Main loop: LED blink + brightness toggle ──
    loop {
        led.set_low();
        Timer::after_millis(1000).await;
        dsi.write_cmd(0, NT35510_CMD_WRDISBV, &[0xFF]).unwrap();

        led.set_high();
        Timer::after_millis(1000).await;
        dsi.write_cmd(0, NT35510_CMD_WRDISBV, &[0x50]).unwrap();
    }
}
