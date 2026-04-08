#![no_std]
#![no_main]

extern crate alloc;

#[cfg(feature = "defmt")]
use defmt_rtt as _;

#[cfg(feature = "defmt")]
use panic_probe as _;

#[cfg(not(feature = "defmt"))]
use panic_halt as _;

use linked_list_allocator::LockedHeap;

#[global_allocator]
static mut HEAP: LockedHeap = LockedHeap::empty();

const HEAP_SIZE: usize = 64 * 1024;
static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

use embassy_stm32::rcc::{
    mux, AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv,
    PllQDiv, PllRDiv, PllSource, Sysclk,
};
use embassy_stm32::Config;
use embassy_stm32f469i_disco::display::SdramCtrl;
use embedded_display_controller::dsi::{DsiHostCtrlIo, DsiWriteCommand, DsiReadCommand};

const GPIOD: usize = 0x40020C00;
const GPIOG: usize = 0x40021800;
const AHB1ENR: usize = 0x40023830;
const DSI_BASE: usize = 0x4001_6C00;
const LTDC_BASE: usize = 0x4000_7400;

unsafe fn led(gpio: usize, pin: u8, on: bool) {
    let moder = core::ptr::read_volatile((gpio) as *const u32);
    core::ptr::write_volatile((gpio) as *mut u32,
        (moder & !(0x3 << (pin * 2))) | (0x1 << (pin * 2)));
    let odr = core::ptr::read_volatile((gpio + 0x14) as *const u32);
    if on {
        core::ptr::write_volatile((gpio + 0x14) as *mut u32, odr | (1 << pin));
    } else {
        core::ptr::write_volatile((gpio + 0x14) as *mut u32, odr & !(1 << pin));
    }
}

unsafe fn setup_leds() {
    let ahb1 = core::ptr::read_volatile(AHB1ENR as *const u32);
    core::ptr::write_volatile(AHB1ENR as *mut u32, ahb1 | (1 << 5) | (1 << 6));
}

unsafe fn reg32(base: usize, offset: usize) -> u32 {
    core::ptr::read_volatile((base + offset) as *const u32)
}
unsafe fn reg32_write(base: usize, offset: usize, val: u32) {
    core::ptr::write_volatile((base + offset) as *mut u32, val);
}
unsafe fn reg32_set(base: usize, offset: usize, val: u32) {
    let old = core::ptr::read_volatile((base + offset) as *const u32);
    core::ptr::write_volatile((base + offset) as *mut u32, old | val);
}
unsafe fn reg32_clear(base: usize, offset: usize, val: u32) {
    let old = core::ptr::read_volatile((base + offset) as *const u32);
    core::ptr::write_volatile((base + offset) as *mut u32, old & !val);
}
unsafe fn reg32_modify(base: usize, offset: usize, f: impl FnOnce(u32) -> u32) {
    let old = core::ptr::read_volatile((base + offset) as *const u32);
    core::ptr::write_volatile((base + offset) as *mut u32, f(old));
}

fn delay_ms(ms: u32) {
    let ticks_per_ms = 180_000_000 / 1000;
    for _ in 0..ms { cortex_m::asm::delay(ticks_per_ms); }
}

struct RawDsi {
    cmd_count: u32,
}

#[derive(defmt::Format)]
enum DsiError {
    FifoFull,
}

impl RawDsi {
    const GHCR: usize = 0x6C;
    const GPDR: usize = 0x70;
    const GPSR: usize = 0x74;
    const ISR1: usize = 0xC4;
    unsafe fn wait_cmd_fifo_empty(&self) -> bool {
        for _ in 0..10_000_000 {
            if reg32(DSI_BASE, Self::GPSR) & 1 != 0 { return true; }
        }
        false
    }
    unsafe fn wait_read_not_busy(&self) -> bool {
        for _ in 0..100_000 {
            if reg32(DSI_BASE, Self::GPSR) & (1 << 4) == 0 { return true; }
        }
        false
    }
    unsafe fn wait_payload_fifo_not_empty(&self) -> bool {
        for _ in 0..100_000 {
            if reg32(DSI_BASE, Self::GPSR) & (1 << 2) == 0 { return true; }
        }
        false
    }
    unsafe fn ghcr_write(&mut self, wcmsb: u8, wclsb: u8, dt: u8) {
        self.wait_cmd_fifo_empty();
        reg32_write(DSI_BASE, Self::GHCR,
            ((wcmsb as u32) << 16) | ((wclsb as u32) << 8) | (dt as u32));
    }
}

impl DsiHostCtrlIo for RawDsi {
    type Error = DsiError;

    fn write(&mut self, cmd: DsiWriteCommand) -> Result<(), Self::Error> {
        self.cmd_count += 1;
        match cmd {
            DsiWriteCommand::DcsShortP0 { arg } => unsafe {
                self.ghcr_write(0, arg, 0x05);
                defmt::trace!("DSI[{}] DcsShortP0 arg={:02x}", self.cmd_count, arg);
                Ok(())
            },
            DsiWriteCommand::DcsShortP1 { arg, data } => unsafe {
                self.ghcr_write(data, arg, 0x15);
                defmt::trace!("DSI[{}] DcsShortP1 arg={:02x} data={:02x}", self.cmd_count, arg, data);
                Ok(())
            },
            DsiWriteCommand::DcsLongWrite { arg, data } => unsafe {
                if !self.wait_cmd_fifo_empty() {
                    defmt::error!("DSI[{}] long write: cmd FIFO not empty before write", self.cmd_count);
                    return Err(DsiError::FifoFull);
                }
                let mut fifoword = arg as u32;
                for (i, byte) in data.iter().take(3).enumerate() {
                    fifoword |= (*byte as u32) << (8 + 8 * i);
                }
                reg32_write(DSI_BASE, Self::GPDR, fifoword);
                if data.len() > 3 {
                    let mut i = 3;
                    while i + 4 <= data.len() {
                        let w: [u8; 4] = data[i..i + 4].try_into().unwrap();
                        reg32_write(DSI_BASE, Self::GPDR, u32::from_ne_bytes(w));
                        i += 4;
                    }
                    let mut fw = 0u32;
                    let mut j = 0;
                    while i < data.len() {
                        fw |= (data[i] as u32) << (j * 8);
                        i += 1;
                        j += 1;
                    }
                    if j > 0 {
                        reg32_write(DSI_BASE, Self::GPDR, fw);
                    }
                }
                let len = (data.len() + 1) as u16;
                self.ghcr_write(((len >> 8) & 0xFF) as u8, (len & 0xFF) as u8, 0x39);
                defmt::trace!("DSI[{}] DcsLongWrite arg={:02x} len={}", self.cmd_count, arg, len);
                if !self.wait_cmd_fifo_empty() {
                    let gpsr = reg32(DSI_BASE, Self::GPSR);
                    let isr1 = reg32(DSI_BASE, Self::ISR1);
                    let isr0 = reg32(DSI_BASE, 0xC0);
                    let pctlr = reg32(DSI_BASE, 0xA0);
                    let cmcr = reg32(DSI_BASE, 0x68);
                    let cr = reg32(DSI_BASE, 0x04);
                    let wcr = reg32(DSI_BASE, 0x404);
                    defmt::error!("DSI[{}] STALL: GPSR={:08x} ISR0={:08x} ISR1={:08x} PCTLR={:08x} CMCR={:08x} CR={:08x} WCR={:08x}", 
                        self.cmd_count, gpsr, isr0, isr1, pctlr, cmcr, cr, wcr);
                    return Err(DsiError::FifoFull);
                }
                Ok(())
            },
            DsiWriteCommand::SetMaximumReturnPacketSize(len) => unsafe {
                self.ghcr_write(((len >> 8) & 0xFF) as u8, (len & 0xFF) as u8, 0x37);
                defmt::trace!("DSI[{}] SetMaxReturnPacketSize len={}", self.cmd_count, len);
                Ok(())
            },
            DsiWriteCommand::GenericShortP1 => unsafe {
                self.ghcr_write(0, 0, 0x13);
                defmt::trace!("DSI[{}] GenericShortP1", self.cmd_count);
                Ok(())
            },
            DsiWriteCommand::GenericShortP2 => unsafe {
                self.ghcr_write(0, 0, 0x23);
                defmt::trace!("DSI[{}] GenericShortP2", self.cmd_count);
                Ok(())
            },
            DsiWriteCommand::GenericLongWrite { arg, data } => unsafe {
                if !self.wait_cmd_fifo_empty() {
                    defmt::error!("DSI[{}] generic long write: cmd FIFO not empty", self.cmd_count);
                    return Err(DsiError::FifoFull);
                }
                let mut fifoword = arg as u32;
                for (i, byte) in data.iter().take(3).enumerate() {
                    fifoword |= (*byte as u32) << (8 + 8 * i);
                }
                reg32_write(DSI_BASE, Self::GPDR, fifoword);
                if data.len() > 3 {
                    let mut i = 3;
                    while i + 4 <= data.len() {
                        let w: [u8; 4] = data[i..i + 4].try_into().unwrap();
                        reg32_write(DSI_BASE, Self::GPDR, u32::from_ne_bytes(w));
                        i += 4;
                    }
                    let mut fw = 0u32;
                    let mut j = 0;
                    while i < data.len() {
                        fw |= (data[i] as u32) << (j * 8);
                        i += 1;
                        j += 1;
                    }
                    if j > 0 {
                        reg32_write(DSI_BASE, Self::GPDR, fw);
                    }
                }
                let len = (data.len() + 1) as u16;
                self.ghcr_write(((len >> 8) & 0xFF) as u8, (len & 0xFF) as u8, 0x29);
                defmt::trace!("DSI[{}] GenericLongWrite arg={:02x} len={}", self.cmd_count, arg, len);
                if !self.wait_cmd_fifo_empty() {
                    defmt::error!("DSI[{}] generic long write: cmd FIFO not empty after trigger", self.cmd_count);
                    return Err(DsiError::FifoFull);
                }
                Ok(())
            },
            DsiWriteCommand::GenericShortP0 => unsafe {
                self.ghcr_write(0, 0, 0x03);
                defmt::trace!("DSI[{}] GenericShortP0", self.cmd_count);
                Ok(())
            },
            _ => {
                defmt::warn!("DSI[{}] unhandled command type", self.cmd_count);
                Ok(())
            },
        }
    }
    fn read(&mut self, cmd: DsiReadCommand, buf: &mut [u8]) -> Result<(), Self::Error> {
        self.cmd_count += 1;
        if buf.len() > 2 && buf.len() <= 65_535 {
            self.write(DsiWriteCommand::SetMaximumReturnPacketSize(buf.len() as u16))?;
        }
        match cmd {
            DsiReadCommand::DcsShort { arg } => unsafe {
                defmt::trace!("DSI[{}] DcsRead arg={:02x} len={}", self.cmd_count, arg, buf.len());
                self.ghcr_write(0, arg, 0x06);
                if !self.wait_read_not_busy() {
                    defmt::error!("DSI[{}] read: timed out waiting for read not busy", self.cmd_count);
                    return Err(DsiError::FifoFull);
                }
                if reg32(DSI_BASE, Self::GPSR) & (1 << 4) == 0
                    && reg32(DSI_BASE, Self::ISR1) & (1 << 6) != 0 {
                    defmt::error!("DSI[{}] read: PSE error (packet size error)", self.cmd_count);
                    return Err(DsiError::FifoFull);
                }
                let mut idx = 0;
                let mut left = buf.len();
                while left > 0 {
                    if !self.wait_payload_fifo_not_empty() {
                    defmt::error!("DSI[{}] read: payload FIFO empty", self.cmd_count);
                    return Err(DsiError::FifoFull);
                }
                    let val = reg32(DSI_BASE, Self::GPDR);
                    let chunk = core::cmp::min(left, 4);
                    for (i, byte) in buf[idx..idx + chunk].iter_mut().enumerate() {
                        *byte = ((val >> (i * 8)) & 0xFF) as u8;
                    }
                    idx += chunk;
                    left -= chunk;
                }
                Ok(())
            },
            _ => {
                defmt::warn!("DSI[{}] unhandled read command type", self.cmd_count);
                Ok(())
            },
        }
    }
}

#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn LTDC() { cortex_m::asm::nop(); }
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn DMA2D() { cortex_m::asm::nop(); }
#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn FMC() { cortex_m::asm::nop(); }

#[allow(non_snake_case)]
#[no_mangle]
unsafe extern "C" fn HardFault() -> ! {
    defmt::error!("HARDFAULT!");
    cortex_m::asm::delay(168_000_000);
    loop {
        led(GPIOD, 5, true);
        for _ in 0..500_000 {}
        led(GPIOD, 5, false);
        for _ in 0..500_000 {}
    }
}

fn step(n: u8) {
    unsafe {
        led(GPIOD, 4, false); led(GPIOD, 5, false); led(GPIOG, 6, false);
        match n {
            1 => { led(GPIOG, 6, true); }
            2 => { led(GPIOD, 4, true); }
            3 => { led(GPIOD, 5, true); }
            4 => { led(GPIOG, 6, true); led(GPIOD, 4, true); }
            5 => { led(GPIOD, 4, true); led(GPIOD, 5, true); }
            6 => { led(GPIOG, 6, true); led(GPIOD, 5, true); }
            7 => { led(GPIOG, 6, true); led(GPIOD, 4, true); led(GPIOD, 5, true); }
            _ => {}
        }
        cortex_m::asm::delay(3_000_000);
        led(GPIOD, 4, false); led(GPIOD, 5, false); led(GPIOG, 6, false);
    }
}

#[embassy_executor::main]
async fn main(_spawner: embassy_executor::Spawner) {
    defmt::info!("=== async_display_test starting ===");
    unsafe {
        HEAP.lock().init(core::ptr::addr_of_mut!(HEAP_MEMORY) as *mut u8, HEAP_SIZE);
    }
    defmt::info!("HEAP initialized");
    unsafe { setup_leds(); }
    defmt::info!("LEDs set up");

    let mut config = Config::default();
    config.rcc.hse = Some(Hse { freq: embassy_stm32::time::mhz(8), mode: HseMode::Oscillator });
    config.rcc.pll_src = PllSource::HSE;
    config.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV4, mul: PllMul::MUL168,
        divp: Some(PllPDiv::DIV2), divq: Some(PllQDiv::DIV7), divr: None,
    });
    config.rcc.sys = Sysclk::PLL1_P;
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV4;
    config.rcc.apb2_pre = APBPrescaler::DIV2;
    config.rcc.mux.clk48sel = mux::Clk48sel::PLL1_Q;
    config.rcc.pllsai = Some(Pll {
        prediv: PllPreDiv::DIV8,
        mul: PllMul::MUL384,
        divp: None,
        divq: Some(PllQDiv::DIV8),
        divr: Some(PllRDiv::DIV7),
    });

    let mut p = embassy_stm32::init(config);
    defmt::info!("STEP 1: embassy init done");
    step(1);

    let sdram = SdramCtrl::new(&mut p, 168_000_000);
    defmt::info!("STEP 2: SDRAM done");
    step(2);

    defmt::info!("STEP 3: calling DisplayCtrl::new()...");
    step(3);
    let mut display = embassy_stm32f469i_disco::DisplayCtrl::new(
        &sdram,
        p.PH7,
        embassy_stm32f469i_disco::BoardHint::ForceNt35510,
    );
    defmt::info!("STEP 4: DisplayCtrl::new() done!");
    step(4);

    display.fb().clear(embedded_graphics::pixelcolor::Rgb565::new(0, 0, 0));
    defmt::info!("STEP 5: framebuffer cleared");

    let fb_ptr = 0xC0000000 as *mut u16;
    unsafe {
        for i in 0..10u32 {
            core::ptr::write_volatile(fb_ptr.add(i as usize), 0xF800);
        }
    }
    cortex_m::asm::delay(168_000);
    let fb0: u16 = unsafe { core::ptr::read_volatile(fb_ptr) };
    let fb1: u16 = unsafe { core::ptr::read_volatile(fb_ptr.add(1)) };
    defmt::info!("SDRAM verify: write 0xF800, read fb[0]={:04x} fb[1]={:04x}", fb0, fb1);

    let pllsaicfgr: u32 = unsafe { core::ptr::read_volatile(0x40023888 as *const u32) };
    let rcc_cr: u32 = unsafe { core::ptr::read_volatile(0x40023800 as *const u32) };
    let dckcfgr: u32 = unsafe { core::ptr::read_volatile(0x4002388C as *const u32) };
    let pllsai_en = (rcc_cr >> 28) & 1;
    let pllsai_rdy = (rcc_cr >> 29) & 1;
    let pllsai_m = pllsaicfgr & 0xF;
    let pllsai_n = (pllsaicfgr >> 6) & 0x1FF;
    let pllsai_r = (pllsaicfgr >> 28) & 0x7;
    let pllsaidivr = (dckcfgr >> 16) & 0x3;
    defmt::info!("PLLSAI: M={} N={} R={} EN={} RDY={} DIVR={}", pllsai_m, pllsai_n, pllsai_r, pllsai_en, pllsai_rdy, pllsaidivr);

    display.fb().clear(embedded_graphics::pixelcolor::Rgb565::new(0, 0, 0));
    defmt::info!("STEP 6: verified and cleared");

    #[cfg(feature = "defmt")]
    defmt::info!("ALL DONE!");

    loop {
        embassy_time::Timer::after_millis(2000).await;
    }
}
