use embassy_stm32::gpio::{AfType, Flex, OutputType, Pull, Speed};
use embassy_stm32::rcc;
use embedded_display_controller::dsi::{DsiHostCtrlIo, DsiReadCommand, DsiWriteCommand};
use embedded_graphics::{
    draw_target::DrawTarget, pixelcolor::Rgb565, prelude::*, primitives::Rectangle,
};
use embedded_hal::delay::DelayNs;
use nt35510::Nt35510;
use stm32_fmc::devices::is42s32400f_6::Is42s32400f6;
use stm32_fmc::{FmcPeripheral, Sdram, SdramTargetBank};

#[allow(dead_code)]
pub const SDRAM_SIZE_BYTES: usize = 16 * 1024 * 1024;
#[allow(dead_code)]
pub const SDRAM_BASE: usize = 0xC000_0000;
pub const FB_WIDTH: u16 = 480;
pub const FB_HEIGHT: u16 = 800;
pub const FB_SIZE: usize = FB_WIDTH as usize * FB_HEIGHT as usize;

const FMC_AF12: AfType = AfType::output_pull(OutputType::PushPull, Speed::VeryHigh, Pull::Up);

const DSI_BASE: usize = 0x4001_6C00;
const LTDC_BASE: usize = 0x4001_6800;

// ── SDRAM ──────────────────────────────────────────────────────────────

struct EmbassyFmc {
    source_clock: u32,
}

unsafe impl Send for EmbassyFmc {}
unsafe impl FmcPeripheral for EmbassyFmc {
    const REGISTERS: *const () = 0xa000_0000 as *const ();
    fn enable(&mut self) {
        rcc::enable_and_reset::<embassy_stm32::peripherals::FMC>();
    }
    fn source_clock_hz(&self) -> u32 {
        self.source_clock
    }
}

fn sdram_pin(pin: embassy_stm32::Peri<'_, impl embassy_stm32::gpio::Pin>) {
    let mut flex = Flex::new(pin);
    flex.set_as_af_unchecked(12, FMC_AF12);
    core::mem::forget(flex);
}

pub struct SdramCtrl {
    mem: *mut u32,
}

impl SdramCtrl {
    pub fn new(p: &mut embassy_stm32::Peripherals, source_clock_hz: u32) -> Self {
        sdram_pin(unsafe { p.PF0.clone_unchecked() });
        sdram_pin(unsafe { p.PF1.clone_unchecked() });
        sdram_pin(unsafe { p.PF2.clone_unchecked() });
        sdram_pin(unsafe { p.PF3.clone_unchecked() });
        sdram_pin(unsafe { p.PF4.clone_unchecked() });
        sdram_pin(unsafe { p.PF5.clone_unchecked() });
        sdram_pin(unsafe { p.PF11.clone_unchecked() });
        sdram_pin(unsafe { p.PF12.clone_unchecked() });
        sdram_pin(unsafe { p.PF13.clone_unchecked() });
        sdram_pin(unsafe { p.PF14.clone_unchecked() });
        sdram_pin(unsafe { p.PF15.clone_unchecked() });
        sdram_pin(unsafe { p.PG0.clone_unchecked() });
        sdram_pin(unsafe { p.PG1.clone_unchecked() });
        sdram_pin(unsafe { p.PG4.clone_unchecked() });
        sdram_pin(unsafe { p.PG5.clone_unchecked() });
        sdram_pin(unsafe { p.PG8.clone_unchecked() });
        sdram_pin(unsafe { p.PG15.clone_unchecked() });
        sdram_pin(unsafe { p.PD0.clone_unchecked() });
        sdram_pin(unsafe { p.PD1.clone_unchecked() });
        sdram_pin(unsafe { p.PD8.clone_unchecked() });
        sdram_pin(unsafe { p.PD9.clone_unchecked() });
        sdram_pin(unsafe { p.PD10.clone_unchecked() });
        sdram_pin(unsafe { p.PD14.clone_unchecked() });
        sdram_pin(unsafe { p.PD15.clone_unchecked() });
        sdram_pin(unsafe { p.PE0.clone_unchecked() });
        sdram_pin(unsafe { p.PE1.clone_unchecked() });
        sdram_pin(unsafe { p.PE7.clone_unchecked() });
        sdram_pin(unsafe { p.PE8.clone_unchecked() });
        sdram_pin(unsafe { p.PE9.clone_unchecked() });
        sdram_pin(unsafe { p.PE10.clone_unchecked() });
        sdram_pin(unsafe { p.PE11.clone_unchecked() });
        sdram_pin(unsafe { p.PE12.clone_unchecked() });
        sdram_pin(unsafe { p.PE13.clone_unchecked() });
        sdram_pin(unsafe { p.PE14.clone_unchecked() });
        sdram_pin(unsafe { p.PE15.clone_unchecked() });
        sdram_pin(unsafe { p.PH2.clone_unchecked() });
        sdram_pin(unsafe { p.PH3.clone_unchecked() });
        sdram_pin(unsafe { p.PH8.clone_unchecked() });
        sdram_pin(unsafe { p.PH9.clone_unchecked() });
        sdram_pin(unsafe { p.PH10.clone_unchecked() });
        sdram_pin(unsafe { p.PH11.clone_unchecked() });
        sdram_pin(unsafe { p.PH12.clone_unchecked() });
        sdram_pin(unsafe { p.PH13.clone_unchecked() });
        sdram_pin(unsafe { p.PH14.clone_unchecked() });
        sdram_pin(unsafe { p.PH15.clone_unchecked() });
        sdram_pin(unsafe { p.PI0.clone_unchecked() });
        sdram_pin(unsafe { p.PI1.clone_unchecked() });
        sdram_pin(unsafe { p.PI2.clone_unchecked() });
        sdram_pin(unsafe { p.PI3.clone_unchecked() });
        sdram_pin(unsafe { p.PI4.clone_unchecked() });
        sdram_pin(unsafe { p.PI5.clone_unchecked() });
        sdram_pin(unsafe { p.PI6.clone_unchecked() });
        sdram_pin(unsafe { p.PI7.clone_unchecked() });
        sdram_pin(unsafe { p.PI9.clone_unchecked() });
        sdram_pin(unsafe { p.PI10.clone_unchecked() });
        sdram_pin(unsafe { p.PC0.clone_unchecked() });

        let fmc = EmbassyFmc {
            source_clock: source_clock_hz,
        };
        let mut sdram: Sdram<EmbassyFmc, Is42s32400f6> =
            Sdram::new_unchecked(fmc, SdramTargetBank::Bank1, Is42s32400f6 {});
        let mut delay = embassy_time::Delay;
        let mem = sdram.init(&mut delay);
        defmt::info!("SDRAM initialized at {:#010x}", mem as usize);
        SdramCtrl { mem }
    }

    #[allow(dead_code)]
    pub fn base_address(&self) -> usize {
        self.mem as usize
    }

    pub fn subslice_mut<T>(&self, offset_bytes: usize, len: usize) -> &'static mut [T] {
        let start = (self.mem as usize) + offset_bytes;
        let end = start + len * core::mem::size_of::<T>();
        assert!(end <= (self.mem as usize) + SDRAM_SIZE_BYTES);
        unsafe { &mut *core::ptr::slice_from_raw_parts_mut(start as *mut T, len) }
    }

    pub fn test_quick(&self) -> bool {
        let words = unsafe { core::slice::from_raw_parts_mut(self.mem as *mut u32, 1024) };
        for word in words.iter_mut() {
            *word = 0xDEAD_BEEF;
        }
        for &word in words.iter() {
            if word != 0xDEAD_BEEF {
                return false;
            }
        }
        for word in words.iter_mut() {
            *word = 0;
        }
        true
    }
}

// ── Raw register helpers ──────────────────────────────────────────────

#[inline(always)]
unsafe fn reg32(base: usize, offset: usize) -> u32 {
    core::ptr::read_volatile((base + offset) as *const u32)
}

#[inline(always)]
unsafe fn reg32_set(base: usize, offset: usize, val: u32) {
    let old = core::ptr::read_volatile((base + offset) as *const u32);
    core::ptr::write_volatile((base + offset) as *mut u32, old | val);
}

#[inline(always)]
unsafe fn reg32_clear(base: usize, offset: usize, val: u32) {
    let old = core::ptr::read_volatile((base + offset) as *const u32);
    core::ptr::write_volatile((base + offset) as *mut u32, old & !val);
}

#[inline(always)]
unsafe fn reg32_write(base: usize, offset: usize, val: u32) {
    core::ptr::write_volatile((base + offset) as *mut u32, val);
}

#[inline(always)]
unsafe fn reg32_modify(base: usize, offset: usize, f: impl FnOnce(u32) -> u32) {
    let old = core::ptr::read_volatile((base + offset) as *const u32);
    core::ptr::write_volatile((base + offset) as *mut u32, f(old));
}

// ── DSI PHY init ──────────────────────────────────────────────────────

#[allow(dead_code)]
unsafe fn dsi_init() {
    // DSIHOST register offsets from stm32-metapac-21.0.0 Dsihost v1
    const CR: usize = 0x04;
    const CCR: usize = 0x08;
    const LVCIDR: usize = 0x0C;
    const LCOLCR: usize = 0x10;
    const LPCR: usize = 0x14;
    const LPMCR: usize = 0x18;
    const PCR: usize = 0x2C;
    const GVCIDR: usize = 0x30;
    const MCR: usize = 0x34;
    const VMCR: usize = 0x38;
    const VPCR: usize = 0x3C;
    const VCCR: usize = 0x40;
    const VNPCR: usize = 0x44;
    const VHSACR: usize = 0x48;
    const VHBPACR: usize = 0x4C;
    const VLCR: usize = 0x50;
    const VVSACR: usize = 0x54;
    const VVBPCR: usize = 0x58;
    const VVFPCR: usize = 0x5C;
    const VVACR: usize = 0x60;
    const LCCR: usize = 0x64;
    const CMCR: usize = 0x68;
    const GHCR: usize = 0x6C;
    const GPDR: usize = 0x70;
    const GPSR: usize = 0x74;
    const CLCR: usize = 0x94;
    const CLTCR: usize = 0x98;
    const DLTCR: usize = 0x9C;
    const PCTLR: usize = 0xA0;
    const PCONFR: usize = 0xA4;
    const IER0: usize = 0xC4;
    const IER1: usize = 0xC8;
    const WRPCR: usize = 0x430;
    const WISR: usize = 0x40C;
    const WCFGR: usize = 0x400;
    const WCR: usize = 0x404;
    const WPCR0: usize = 0x418;

    // Timing from sync BSP (NT35510_DISPLAY_CONFIG)
    let h_sync = 2u32;
    let h_back_porch = 34u32;
    let h_front_porch = 34u32;
    let v_sync = 1u32;
    let v_back_porch = 15u32;
    let v_front_porch = 16u32;
    let active_width = FB_WIDTH as u32; // 480
    let active_height = FB_HEIGHT as u32; // 800
    let lane_byte_clk = 500_000_000u32; // 500MHz (VCO/ODF=500/1)
    let pixel_clk = 27_429u32; // ~27.4 MHz (from sync BSP)

    // Shutdown
    reg32_clear(DSI_BASE, CR, 1 << 2); // CMDM=0
    reg32_clear(DSI_BASE, WCFGR, 1 << 0); // DSIM=0
    reg32_clear(DSI_BASE, CR, 1 << 0); // EN=0
    reg32_write(DSI_BASE, PCTLR, 0); // CKE=0, DEN=0
    reg32_clear(DSI_BASE, WRPCR, 1 << 0); // PLLEN=0
    reg32_clear(DSI_BASE, WRPCR, 1 << 24); // REGEN=0

    cortex_m::asm::delay(168_000);

    // Enable DSIHOST and LTDC peripheral clocks
    // DSIHOST: APB2, bit 27 (RCC.APB2ENR)
    // LTDC: APB2, bit 26 (RCC.APB2ENR)
    // RCC base = 0x4002_3800, APB2ENR offset = 0x44
    unsafe {
        let apb2enr_addr = 0x4002_3844usize;
        let apb2enr = core::ptr::read_volatile(apb2enr_addr as *const u32);
        core::ptr::write_volatile(apb2enr_addr as *mut u32, apb2enr | (1 << 27) | (1 << 26));
    }
    cortex_m::asm::delay(168_000);

    // Regulator enable (bit 24)
    reg32_set(DSI_BASE, WRPCR, 1 << 24); // REGEN=1
    let mut timeout = 100_000u32;
    while reg32(DSI_BASE, WISR) & (1 << 12) == 0 && timeout > 0 {
        timeout -= 1;
    }
    defmt::assert!(timeout > 0, "DSI regulator timeout");

    // PLL: VCO = (8MHz / IDF=2) * NDIV=125 = 500MHz, LaneByteClk = 500MHz/ODF1 = 500MHz
    // NDIV[8:2], IDF[14:11], ODF[17:16]
    reg32_modify(DSI_BASE, WRPCR, |w| {
        (w & !(0x7F << 2 | 0x0F << 11 | 0x03 << 16))
        | (125 << 2)    // NDIV=125
        | (0x02 << 11)  // IDF=2
        | (0x00 << 16) // ODF=1
    });
    reg32_set(DSI_BASE, WRPCR, 1 << 0); // PLLEN=1

    cortex_m::asm::delay(168_000 / 2); // 400us delay before checking lock

    timeout = 100_000u32;
    while reg32(DSI_BASE, WISR) & (1 << 8) == 0 && timeout > 0 {
        timeout -= 1;
    }
    defmt::assert!(timeout > 0, "DSI PLL lock timeout");

    // PHY params
    reg32_set(DSI_BASE, PCTLR, 1 << 0 | 1 << 1); // CKE=1, DEN=1
    reg32_modify(DSI_BASE, CLCR, |w| w | (1 << 0)); // DPCC=1, ACR=0
    reg32_modify(DSI_BASE, PCONFR, |w| (w & !0x03) | 0x01); // NL=1 (2 data lanes)
    reg32_write(DSI_BASE, CCR, 4); // TXECKDIV=4
    reg32_write(DSI_BASE, WPCR0, 8); // UIX4=8
    reg32_write(DSI_BASE, IER0, 0);
    reg32_write(DSI_BASE, IER1, 0);
    reg32_set(DSI_BASE, PCR, 1 << 2); // BTAE=1

    // Video mode: burst (matching sync BSP exactly)
    reg32_clear(DSI_BASE, CR, 1 << 2); // CMDM=0
    reg32_clear(DSI_BASE, WCFGR, 1 << 0); // DSIM=0 (video mode, NOT command mode)
    reg32_modify(DSI_BASE, VMCR, |w| (w & !0x03) | 0x02); // VMT=2 (burst)
    reg32_write(DSI_BASE, VPCR, active_width); // VPSIZE=480
    reg32_write(DSI_BASE, VCCR, 0); // NUMC=0
    reg32_write(DSI_BASE, VNPCR, 0); // NPSIZE=0
    reg32_write(DSI_BASE, LVCIDR, 0); // VCID=0
    reg32_write(DSI_BASE, LPCR, 0); // DEP=0, HSP=0, VSP=0
    reg32_write(DSI_BASE, LCOLCR, 0x00); // COLC=SixteenBitsConfig1 (RGB565)
    reg32_modify(DSI_BASE, WCFGR, |w| (w & !(0x07 << 1)) | (0x00 << 1)); // COLMUX=SixteenBitsConfig1

    // DSI timing (matching sync BSP calculations)
    // HSA = h_sync * lane_byte_clk / pixel_clk = 2 * 500M / 27429 = ~36500 → fits in u16
    let dsi_hsa =
        ((h_sync as u64 * lane_byte_clk as u64 + pixel_clk as u64 / 2) / pixel_clk as u64) as u32;
    let dsi_hbp = ((h_back_porch as u64 * lane_byte_clk as u64 + pixel_clk as u64 / 2)
        / pixel_clk as u64) as u32;
    let dsi_hline = (((active_width + h_sync + h_back_porch + h_front_porch) as u64
        * lane_byte_clk as u64
        + pixel_clk as u64 / 2)
        / pixel_clk as u64) as u32;
    let dsi_vsa =
        ((v_sync as u64 * lane_byte_clk as u64 + pixel_clk as u64 / 2) / pixel_clk as u64) as u32;
    let dsi_vbp = ((v_back_porch as u64 * lane_byte_clk as u64 + pixel_clk as u64 / 2)
        / pixel_clk as u64) as u32;
    let dsi_vfp = ((v_front_porch as u64 * lane_byte_clk as u64 + pixel_clk as u64 / 2)
        / pixel_clk as u64) as u32;

    reg32_write(DSI_BASE, VHSACR, dsi_hsa);
    reg32_write(DSI_BASE, VHBPACR, dsi_hbp);
    reg32_write(DSI_BASE, VLCR, dsi_hline);
    reg32_write(DSI_BASE, VVSACR, dsi_vsa);
    reg32_write(DSI_BASE, VVBPCR, dsi_vbp);
    reg32_write(DSI_BASE, VVFPCR, dsi_vfp);
    reg32_write(DSI_BASE, VVACR, active_height); // VA=800

    // LP command enable + all LP transitions (matching sync BSP)
    reg32_set(
        DSI_BASE,
        VMCR,
        1 << 20 | // LPCE
        1 << 6  | // LPHFPE
        1 << 7  | // LPHBPE
        1 << 8  | // LPVAE
        1 << 9  | // LPVFPE
        1 << 10 | // LPVBPE
        1 << 11, // LPVSAE
    );

    reg32_write(DSI_BASE, LPMCR, (64 << 0) | (64 << 8)); // LPSIZE=64, VLPSIZE=64

    // HS/LP transition timers
    reg32_write(DSI_BASE, CLTCR, (35 << 0) | (35 << 16)); // HS2LP_TIME, LP2HS_TIME
                                                          // DLTCR: [26:16]=MRD_TIME, [15:8]=LP2HS_TIME, [7:0]=HS2LP_TIME
    reg32_write(DSI_BASE, DLTCR, (35 << 0) | (35 << 8) | (0 << 16)); // HS2LP=35, LP2HS=35, MRD=0
    reg32_modify(DSI_BASE, PCONFR, |w| (w & !0x1F << 16) | (10 << 16)); // SW_TIME=10

    cortex_m::asm::delay(168_000 * 10);

    // Enable DSI host and wrapper
    reg32_set(DSI_BASE, CR, 1 << 0); // EN=1
                                     // WCFGR DSIM stays 0 (video mode) — do NOT set to 1
    reg32_set(DSI_BASE, WCR, 1 << 0); // DSIEN=1 (enable DSI wrapper)
}

// ── LTDC init ─────────────────────────────────────────────────────────

#[allow(dead_code)]
unsafe fn ltdc_init(fb_addr: u32) {
    const GCR: usize = 0x18;
    const SSCR: usize = 0x08;
    const BPCR: usize = 0x0C;
    const AWCR: usize = 0x10;
    const TWCR: usize = 0x14;
    const BCCR: usize = 0x2C;
    const IER: usize = 0x34;
    const SRCR: usize = 0x24;
    // Layer 1 registers (base at 0x84, each field relative to layer base)
    const L1_BASE: usize = 0x84;
    const L1CR: usize = L1_BASE + 0x00;
    const L1WHPCR: usize = L1_BASE + 0x04;
    const L1WVPCR: usize = L1_BASE + 0x08;
    const L1PFCR: usize = L1_BASE + 0x10;
    const L1CACR: usize = L1_BASE + 0x14;
    const L1CFBAR: usize = L1_BASE + 0x28;
    const L1CFBLR: usize = L1_BASE + 0x2C;
    const L1CFBLNR: usize = L1_BASE + 0x30;

    // Timing matching sync BSP (NT35510_DISPLAY_CONFIG)
    let h_sync = 2u32;
    let h_back_porch = 34u32;
    let h_front_porch = 34u32;
    let v_sync = 1u32;
    let v_back_porch = 15u32;
    let v_front_porch = 16u32;

    // Global config
    reg32_write(
        LTDC_BASE,
        GCR,
        (1 << 31) | // HSPOL=ACTIVEHIGH
        (1 << 30) | // VSPOL=ACTIVEHIGH
        (1 << 28), // PCPOL=RISINGEDGE
                   // DEPOL=0 (ACTIVELOW), LTDCEN=0, DEN=0
    );

    reg32_write(
        LTDC_BASE,
        SSCR,
        ((h_sync - 1) & 0xFFF) | (((v_sync - 1) & 0xFFF) << 16),
    );

    reg32_write(
        LTDC_BASE,
        BPCR,
        ((h_sync + h_back_porch - 1) & 0xFFF) | (((v_sync + v_back_porch - 1) & 0xFFF) << 16),
    );

    reg32_write(
        LTDC_BASE,
        AWCR,
        ((FB_WIDTH as u32 + h_sync + h_back_porch - 1) & 0xFFF)
            | (((v_sync + v_back_porch + FB_HEIGHT as u32 - 1) & 0xFFF) << 16),
    );

    reg32_write(
        LTDC_BASE,
        TWCR,
        ((FB_WIDTH as u32 + h_sync + h_back_porch + h_front_porch - 1) & 0xFFF)
            | (((v_sync + v_back_porch + FB_HEIGHT as u32 + v_front_porch - 1) & 0xFFF) << 16),
    );

    reg32_write(LTDC_BASE, BCCR, 0); // Black background
    reg32_write(LTDC_BASE, IER, (1 << 2) | (1 << 1)); // TERRIE, FUIE

    // Layer 1 window
    let ahbp = h_sync + h_back_porch - 1;
    let avbp = v_sync + v_back_porch - 1;
    let bytes_per_pixel: u32 = 2; // RGB565
    let line_length = FB_WIDTH as u32 * bytes_per_pixel;

    reg32_write(
        LTDC_BASE,
        L1WHPCR,
        ((ahbp + 1) & 0xFFF) | (((ahbp + FB_WIDTH as u32) & 0xFFF) << 16),
    );
    reg32_write(
        LTDC_BASE,
        L1WVPCR,
        ((avbp + 1) & 0xFFF) | (((avbp + FB_HEIGHT as u32) & 0xFFF) << 16),
    );
    reg32_write(LTDC_BASE, L1PFCR, 0x02); // RGB565
    reg32_write(LTDC_BASE, L1CACR, 255); // CONSTA=255
    reg32_write(LTDC_BASE, L1CFBAR, fb_addr);
    reg32_write(LTDC_BASE, L1CFBLR, (line_length + 3) | (line_length << 16));
    reg32_write(LTDC_BASE, L1CFBLNR, FB_HEIGHT as u32);
    reg32_write(LTDC_BASE, L1CR, 1 << 0); // LEN=1 (enable layer)

    // Reload and enable
    reg32_write(LTDC_BASE, SRCR, 0x01); // IMR=reload
    while reg32(LTDC_BASE, SRCR) & 0x01 != 0 {}

    reg32_set(LTDC_BASE, GCR, 1 << 0); // LTDCEN=1
}

// ── DsiHostCtrlIo adapter (raw FIFO writes) ────────────────────────────

struct RawDsi;

impl RawDsi {
    const GHCR: usize = 0x6C;
    const GPDR: usize = 0x70;
    const GPDLR: usize = 0x4C8;
}

impl DsiHostCtrlIo for RawDsi {
    type Error = ();

    fn write(&mut self, cmd: DsiWriteCommand) -> Result<(), Self::Error> {
        match cmd {
            DsiWriteCommand::DcsShortP1 { arg, data } => unsafe {
                reg32_write(
                    DSI_BASE,
                    Self::GPDR,
                    0x15 | ((arg as u32) << 8) | ((data as u32) << 16),
                );
            },
            DsiWriteCommand::DcsLongWrite { arg, data } => {
                unsafe {
                    reg32_write(DSI_BASE, Self::GPDR, 0x39 | ((arg as u32) << 8));
                    // Write payload bytes in pairs via GPDLR
                    let mut i = 0;
                    while i < data.len() {
                        let b0 = data[i];
                        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
                        reg32_write(DSI_BASE, Self::GPDLR, ((b1 as u32) << 8) | (b0 as u32));
                        i += 2;
                    }
                    if data.len() % 2 != 0 {
                        reg32_write(DSI_BASE, Self::GPDLR, 0);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn read(&mut self, cmd: DsiReadCommand, buf: &mut [u8]) -> Result<(), Self::Error> {
        match cmd {
            DsiReadCommand::DcsShort { arg } => {
                unsafe {
                    // Set max return packet size
                    reg32_write(
                        DSI_BASE,
                        Self::GPDR,
                        0x37 | (((buf.len() >> 8) & 0xFF) as u32) << 16
                            | ((buf.len() & 0xFF) as u32) << 24,
                    );
                    // Send read command
                    reg32_write(DSI_BASE, Self::GHCR, 0x06 | ((arg as u32) << 8));
                    // Read response bytes
                    for byte in buf.iter_mut() {
                        let val = reg32(DSI_BASE, Self::GPDLR);
                        *byte = (val & 0xFF) as u8;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

// ── Display init (orchestrator) ────────────────────────────────────────

pub struct DisplayCtrl {
    framebuffer: &'static mut [u16],
}

impl DisplayCtrl {
    pub fn new(
        sdram: &SdramCtrl,
        lcd_reset: embassy_stm32::Peri<'_, impl embassy_stm32::gpio::Pin>,
    ) -> Self {
        // LCD reset
        let mut reset_pin = embassy_stm32::gpio::Output::new(
            lcd_reset,
            embassy_stm32::gpio::Level::Low,
            embassy_stm32::gpio::Speed::Low,
        );
        embassy_time::Delay.delay_ms(20);
        reset_pin.set_high();
        embassy_time::Delay.delay_ms(140);
        core::mem::forget(reset_pin);

        // DSI PHY init
        defmt::info!("DSI: PHY init...");
        unsafe {
            dsi_init();
        }

        // LTDC init
        let fb_slice: &'static mut [u16] = sdram.subslice_mut(0, FB_SIZE);
        let fb_addr = fb_slice.as_mut_ptr() as u32;
        defmt::info!("LTDC: init, fb={:#010x}", fb_addr);
        unsafe {
            ltdc_init(fb_addr);
        }

        // nt35510 panel init
        defmt::info!("NT35510: panel init...");
        embassy_time::Delay.delay_ms(120);
        let mut panel = Nt35510::new();
        let mut dsi_adapter = RawDsi;
        let mut delay = embassy_time::Delay;
        panel
            .init_rgb565(&mut dsi_adapter, &mut delay)
            .expect("NT35510 init failed");

        defmt::info!("Display initialized ({}x{} RGB565)", FB_WIDTH, FB_HEIGHT);
        DisplayCtrl {
            framebuffer: fb_slice,
        }
    }

    pub fn fb(&mut self) -> FramebufferView<'_> {
        FramebufferView {
            buffer: self.framebuffer,
        }
    }
}

pub struct FramebufferView<'a> {
    buffer: &'a mut [u16],
}

impl<'a> FramebufferView<'a> {
    pub fn clear(&mut self, color: Rgb565) {
        let raw = color.into_storage();
        for pixel in self.buffer.iter_mut() {
            *pixel = raw;
        }
    }
}

impl<'a> DrawTarget for FramebufferView<'a> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics::Pixel<Self::Color>>,
    {
        for pixel in pixels {
            let x = pixel.0.x as usize;
            let y = pixel.0.y as usize;
            if x < FB_WIDTH as usize && y < FB_HEIGHT as usize {
                self.buffer[y * FB_WIDTH as usize + x] = pixel.1.into_storage();
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, color: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let top = area.top_left.y.max(0) as usize;
        let bottom = (area.top_left.y + area.size.height as i32).min(FB_HEIGHT as i32) as usize;
        let left = area.top_left.x.max(0) as usize;
        let right = (area.top_left.x + area.size.width as i32).min(FB_WIDTH as i32) as usize;

        let flat_color = color.into_iter().next().unwrap_or(Rgb565::BLACK);
        let raw = flat_color.into_storage();

        for y in top..bottom {
            for x in left..right {
                self.buffer[y * FB_WIDTH as usize + x] = raw;
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.clear(color);
        Ok(())
    }
}

impl<'a> OriginDimensions for FramebufferView<'a> {
    fn size(&self) -> Size {
        Size::new(FB_WIDTH as u32, FB_HEIGHT as u32)
    }
}
