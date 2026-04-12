#![no_std]
#![no_main]

#[cfg(feature = "scanner-async")]
use core::ptr::read_volatile;

#[cfg(feature = "scanner-async")]
use cortex_m_rt::entry;
#[cfg(feature = "scanner-async")]
use defmt_rtt as _;
#[cfg(feature = "scanner-async")]
use panic_probe as _;

#[cfg(feature = "scanner-async")]
use embassy_stm32::{
    gpio::{Level, Output, Speed},
    interrupt::InterruptExt,
    mode::Blocking,
    usart, Config,
};
#[cfg(feature = "scanner-async")]
use embedded_hal_02::blocking::serial::Write as _;

#[allow(dead_code)]
mod async_shared {
    #[cfg(feature = "scanner-async")]
    include!("../async_shared.rs");
}

#[cfg(feature = "scanner-async")]
const CPU_HZ_DEFAULT: u32 = 16_000_000;
#[cfg(feature = "scanner-async")]
static mut CPU_HZ: u32 = CPU_HZ_DEFAULT;
#[cfg(feature = "scanner-async")]
const POLL_DELAY_US: u32 = 100;
#[cfg(feature = "scanner-async")]
const GET_SETTING_CMD: [u8; 9] = [0x7E, 0x00, 0x07, 0x01, 0x00, 0x00, 0x01, 0xAB, 0xCD];
#[cfg(feature = "scanner-async")]
const SET_BAUD_115200_CMD: [u8; 10] = [0x7E, 0x00, 0x08, 0x02, 0x00, 0x2A, 0x1A, 0x00, 0xAB, 0xCD];
#[cfg(feature = "scanner-async")]
const BAUD_RATES: [(u32, u8); 5] = [
    (9600, 0x00),
    (19200, 0x01),
    (38400, 0x02),
    (57600, 0x03),
    (115200, 0x1A),
];
#[cfg(feature = "scanner-async")]
const RESPONSE_LEN: usize = 7;
#[cfg(feature = "scanner-async")]
const USART6_SR: *const u32 = 0x4001_1400 as *const u32;
#[cfg(feature = "scanner-async")]
const USART6_DR: *const u32 = 0x4001_1404 as *const u32;
#[cfg(feature = "scanner-async")]
const USART_SR_RXNE: u32 = 1 << 5;
#[cfg(feature = "scanner-async")]
const USART6_BRR: *const u32 = 0x4001_1408 as *const u32;
#[cfg(feature = "scanner-async")]
const RCC_CR: *const u32 = 0x4002_3800 as *const u32;
#[cfg(feature = "scanner-async")]
const RCC_PLLCFGR: *const u32 = 0x4002_3804 as *const u32;
#[cfg(feature = "scanner-async")]
const RCC_CFGR: *const u32 = 0x4002_3808 as *const u32;
#[cfg(feature = "scanner-async")]
const FLASH_ACR: *const u32 = 0x4002_3C00 as *const u32;

#[cfg(feature = "scanner-async")]
fn set_cpu_hz(hz: u32) {
    // SAFETY: single-threaded bare-metal — no concurrent access to CPU_HZ
    unsafe { CPU_HZ = hz };
}

#[cfg(feature = "scanner-async")]
fn update_usart_brr(apb2_hz: u32, baud: u32) {
    let usartdiv = (apb2_hz + baud / 2) / baud;
    // SAFETY: USART6 BRR is valid for 115200 baud at this SYSCLK
    unsafe {
        core::ptr::write_volatile(USART6_BRR as *mut u32, usartdiv);
    }
    defmt::info!(
        "USART6 BRR set to 0x{:04x} ({}Hz / {} = {})",
        usartdiv,
        apb2_hz,
        baud,
        usartdiv
    );
}

#[cfg(feature = "scanner-async")]
fn delay_us(us: u32) {
    // SAFETY: single-threaded bare-metal
    let hz = unsafe { CPU_HZ };
    cortex_m::asm::delay((hz / 1_000_000) * us);
}

#[cfg(feature = "scanner-async")]
fn delay_ms(ms: u32) {
    for _ in 0..ms {
        delay_us(1_000);
    }
}

#[cfg(feature = "scanner-async")]
fn write_all_blocking(
    uart: &mut usart::Uart<'_, Blocking>,
    bytes: &[u8],
) -> Result<(), usart::Error> {
    uart.bwrite_all(bytes)?;
    uart.bflush()?;
    Ok(())
}

#[cfg(feature = "scanner-async")]
fn uart_rx_ready() -> bool {
    // SAFETY: USART6_SR at 0x4001_1400 (RM0090 table 45)
    unsafe { read_volatile(USART6_SR) & USART_SR_RXNE != 0 }
}

#[cfg(feature = "scanner-async")]
fn uart_read_byte() -> u8 {
    // SAFETY: USART6_DR at 0x4001_1404 (RM0090 table 44)
    unsafe { read_volatile(USART6_DR as *const u8) }
}

#[cfg(feature = "scanner-async")]
fn drain_uart_rx() {
    while uart_rx_ready() {
        let _ = uart_read_byte();
    }
}

#[cfg(feature = "scanner-async")]
fn read_response(buf: &mut [u8], timeout_ms: u32) -> usize {
    let polls = timeout_ms.saturating_mul(1_000 / POLL_DELAY_US);
    let mut len = 0;

    for _ in 0..polls {
        while uart_rx_ready() && len < buf.len() {
            buf[len] = uart_read_byte();
            len += 1;
        }

        if len >= RESPONSE_LEN {
            return len;
        }

        delay_us(POLL_DELAY_US);
    }

    while uart_rx_ready() && len < buf.len() {
        buf[len] = uart_read_byte();
        len += 1;
    }

    len
}

#[cfg(feature = "scanner-async")]
fn probe_current_baud(uart: &mut usart::Uart<'_, Blocking>, baud: u32) -> bool {
    defmt::info!("Trying {} baud", baud);
    drain_uart_rx();

    if let Err(err) = write_all_blocking(uart, &GET_SETTING_CMD) {
        defmt::warn!("Write failed at {} baud: {:?}", baud, err);
        return false;
    }

    let mut response = [0u8; 16];
    let len = read_response(&mut response, 100);

    if len > 0 && response[0] == 0x02 {
        defmt::info!("Scanner responded at {} baud", baud);
        return true;
    }

    if len == 0 {
        defmt::warn!("No response at {} baud", baud);
    } else {
        defmt::warn!(
            "Unexpected response at {} baud: len={}, first=0x{:02x}",
            baud,
            len,
            response[0]
        );
    }

    false
}

#[cfg(feature = "scanner-async")]
fn blink_forever(led: &mut Output<'_>, on_ms: u32, off_ms: u32) -> ! {
    loop {
        led.set_high();
        delay_ms(on_ms);
        led.set_low();
        delay_ms(off_ms);
    }
}

#[cfg(feature = "scanner-async")]
fn switch_to_pll_hse() -> u32 {
    // SAFETY: defmt diagnostic — directly manipulates RCC/PLL/FLASH registers
    unsafe {
        let rcc_cr = read_volatile(RCC_CR);
        let hserdy = (rcc_cr >> 17) & 1;
        if hserdy == 0 {
            core::ptr::write_volatile(RCC_CR as *mut u32, rcc_cr | (1 << 16));
            while read_volatile(RCC_CR) & (1 << 17) == 0 {}
            defmt::info!("HSE ready");
        }

        let pllcfgr: u32 =
            (4 & 0x3F) | ((168 & 0x1FF) << 6) | (0 << 16) | (1 << 22) | ((7 & 0xF) << 24);
        core::ptr::write_volatile(RCC_PLLCFGR as *mut u32, pllcfgr);
        core::ptr::write_volatile(RCC_CR as *mut u32, read_volatile(RCC_CR) | (1 << 24));
        while read_volatile(RCC_CR) & (1 << 25) == 0 {}
        defmt::info!("PLL ready");

        core::ptr::write_volatile(
            FLASH_ACR as *mut u32,
            (read_volatile(FLASH_ACR) & !(0xF)) | 5,
        );
        while (read_volatile(FLASH_ACR) & 0xF) != 5 {}
        defmt::info!("Flash latency set to 5WS");

        let cfgr = read_volatile(RCC_CFGR);
        let sw_pll = 2u32;
        let apb2_div2 = (4u32) << 13;
        let apb1_div4 = (5u32) << 10;
        let new_cfgr = (cfgr & !(0x3)) | sw_pll | apb2_div2 | apb1_div4;
        core::ptr::write_volatile(RCC_CFGR as *mut u32, new_cfgr);
        while (read_volatile(RCC_CFGR) >> 2 & 0x3) != 2 {}
        defmt::info!("SYSCLK switched to PLL");
    }

    set_cpu_hz(168_000_000);
    defmt::info!("Switched to PLL: SYSCLK=168MHz, APB2=84MHz");
    84_000_000
}

#[cfg(feature = "scanner-async")]
#[entry]
fn main() -> ! {
    defmt::info!("========================================");
    defmt::info!("GM65 baud probe");
    defmt::info!("========================================");

    let config = Config::default();
    let p = embassy_stm32::init(config);

    let mut led_green = Output::new(p.PG6, Level::Low, Speed::Low);
    let mut led_red = Output::new(p.PD5, Level::Low, Speed::Low);
    let mut led_blue = Output::new(p.PK3, Level::Low, Speed::Low);

    let mut uart_config = usart::Config::default();
    uart_config.baudrate = BAUD_RATES[0].0;

    let mut uart = match usart::Uart::new_blocking(p.USART6, p.PG9, p.PG14, uart_config) {
        Ok(uart) => uart,
        Err(err) => {
            defmt::error!("Failed to create USART6: {:?}", err);
            blink_forever(&mut led_red, 200, 200);
        }
    };
    embassy_stm32::interrupt::USART6.disable();

    let mut found_baud = None;

    for (baud, _code) in BAUD_RATES {
        if let Err(err) = uart.set_baudrate(baud) {
            defmt::warn!("Failed to reconfigure USART6 to {} baud: {:?}", baud, err);
            continue;
        }

        delay_ms(10);

        if probe_current_baud(&mut uart, baud) {
            found_baud = Some(baud);
            break;
        }
    }

    let Some(found_baud) = found_baud else {
        defmt::error!("Scanner not found at any supported baud rate");
        blink_forever(&mut led_red, 100, 100);
    };

    if found_baud != 115200 {
        defmt::warn!("Scanner found at {} baud, switching to 115200", found_baud);

        if let Err(err) = write_all_blocking(&mut uart, &SET_BAUD_115200_CMD) {
            defmt::error!("Failed to send set-baud command: {:?}", err);
            blink_forever(&mut led_red, 100, 100);
        }

        delay_ms(100);

        if let Err(err) = uart.set_baudrate(115200) {
            defmt::error!("Failed to reconfigure USART6 to 115200: {:?}", err);
            blink_forever(&mut led_red, 100, 100);
        }

        delay_ms(10);

        if !probe_current_baud(&mut uart, 115200) {
            defmt::error!("Scanner did not respond after switching to 115200");
            blink_forever(&mut led_red, 100, 100);
        }
    } else {
        defmt::info!("Scanner already communicating at 115200 baud");
    }

    defmt::info!(
        "Final status: scanner found at {} baud, now communicating at 115200",
        found_baud
    );

    defmt::info!("");
    defmt::info!("=== PHASE 2: Test with PLL clock ===");
    defmt::info!("Scanner confirmed working at HSI 16MHz",);
    defmt::info!("Now switching to HSE PLL: SYSCLK=168MHz, APB2=84MHz",);

    let apb2_hz = switch_to_pll_hse();
    update_usart_brr(apb2_hz, 115200);
    delay_ms(10);

    let mut response = [0u8; 16];
    drain_uart_rx();
    if let Err(err) = write_all_blocking(&mut uart, &GET_SETTING_CMD) {
        defmt::error!("Write failed after PLL switch: {:?}", err);
        blink_forever(&mut led_red, 100, 100);
    }

    let len = read_response(&mut response, 200);
    if len > 0 && response[0] == 0x02 {
        defmt::info!("Scanner STILL responds at PLL 84MHz APB2! Response: {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}", response[0], response[1], response[2], response[3], response[4], response[5], response[6]);
        defmt::info!("PLL clock config is NOT the issue. Something else is wrong.");
        blink_forever(&mut led_blue, 500, 500);
    } else if len == 0 {
        defmt::error!("Scanner STOPPED responding after PLL switch! (0 bytes received)");
        defmt::error!("This confirms PLL clock config breaks UART communication.");
        blink_forever(&mut led_red, 200, 200);
    } else {
        defmt::error!(
            "Garbage response after PLL switch: len={}, first=0x{:02x}",
            len,
            response[0]
        );
        blink_forever(&mut led_red, 300, 300);
    }
}

#[cfg(not(feature = "scanner-async"))]
use cortex_m_rt::entry;
#[cfg(not(feature = "scanner-async"))]
use defmt_rtt as _;
#[cfg(not(feature = "scanner-async"))]
use panic_probe as _;

#[cfg(not(feature = "scanner-async"))]
#[entry]
fn main() -> ! {
    defmt::error!("This binary requires the 'scanner-async' feature");
    loop {
        cortex_m::asm::wfi();
    }
}
