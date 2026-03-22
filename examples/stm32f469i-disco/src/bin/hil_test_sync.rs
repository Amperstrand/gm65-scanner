//! HIL Test Binary for Sync Driver
//!
//! Run with: cargo run --target thumbv7em-none-eabihf --bin hil_test_sync

#![no_std]
#![no_main]

extern crate alloc;

use cortex_m_rt::entry;
use defmt_rtt as _;
use panic_probe as _;

use gm65_scanner::{driver::hil_tests, Gm65Scanner, ScannerDriverSync};
use linked_list_allocator::LockedHeap;
use stm32f469i_disc::{hal::pac, hal::prelude::*, hal::rcc, hal::serial::Serial6};

const HEAP_SIZE: usize = 32 * 1024;
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[entry]
fn main() -> ! {
    unsafe {
        ALLOCATOR
            .lock()
            .init(core::ptr::addr_of_mut!(HEAP_MEMORY) as *mut u8, HEAP_SIZE);
    }

    let dp = pac::Peripherals::take().unwrap();

    let mut rcc = dp.RCC.freeze(
        rcc::Config::hse(8.MHz())
            .pclk2(32.MHz())
            .sysclk(180.MHz())
            .require_pll48clk(),
    );

    let gpiog = dp.GPIOG.split(&mut rcc);
    let scanner_tx = gpiog.pg14;
    let scanner_rx = gpiog.pg9;

    let baud_rates: [u32; 3] = [9600, 57600, 115200];
    let mut scanner: Option<Gm65Scanner<Serial6>> = None;
    let mut scanner_usart = Some(dp.USART6);
    let mut scanner_pins = Some((scanner_tx, scanner_rx));

    for &baud in &baud_rates {
        let (usart, pins) = match (scanner_usart.take(), scanner_pins.take()) {
            (Some(u), Some(p)) => (u, p),
            _ => break,
        };
        let uart = usart.serial(pins, baud.bps(), &mut rcc).unwrap();
        let mut s = Gm65Scanner::with_default_config(uart);
        defmt::info!("Probing scanner at {} bps (init)...", baud);
        match s.init() {
            Ok(model) => {
                defmt::info!("Scanner found at {} bps, model={:?}", baud, model);
                scanner = Some(s);
                break;
            }
            Err(_) => {
                defmt::info!("No response at {} bps, trying next...", baud);
                let (raw_usart, raw_pins) = s.release().release();
                scanner_usart = Some(raw_usart);
                let tx_pin: stm32f469i_disc::hal::gpio::Pin<'G', 14> =
                    raw_pins.0.unwrap().try_into().ok().unwrap();
                let rx_pin: stm32f469i_disc::hal::gpio::Pin<'G', 9> =
                    raw_pins.1.unwrap().try_into().ok().unwrap();
                scanner_pins = Some((tx_pin, rx_pin));
            }
        }
    }

    let mut scanner = match scanner {
        Some(s) => s,
        None => {
            let (usart, pins) = match (scanner_usart.take(), scanner_pins.take()) {
                (Some(u), Some(p)) => (u, p),
                _ => panic!("No USART6 available"),
            };
            let uart = usart.serial(pins, 9600.bps(), &mut rcc).unwrap();
            let s = Gm65Scanner::with_default_config(uart);
            defmt::warn!("QR scanner not found at any baud rate, using 9600 default");
            s
        }
    };

    defmt::info!("Running HIL tests (sync)...");
    let results = hil_tests::run_hil_tests(&mut scanner);

    if results.all_passed() {
        defmt::info!("All HIL tests passed!");
    } else {
        defmt::error!("HIL tests failed: {}/5", results.passed_count());
    }

    loop {}
}
