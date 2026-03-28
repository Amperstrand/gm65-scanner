//! HIL Test Binary for Sync Driver
//!
//! Run with: cargo run --target thumbv7em-none-eabihf --bin hil_test_sync

#![no_std]
#![no_main]

extern crate alloc;

use cortex_m::asm::delay;
use cortex_m_rt::entry;
use defmt_rtt as _;
use panic_probe as _;

use gm65_scanner::{driver::hil_tests, Gm65Scanner, ScannerDriverSync, ScannerSettings};
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

    let uart = Serial6::new(dp.USART6, (scanner_tx, scanner_rx), 115200.bps(), &mut rcc).unwrap();
    let mut scanner = Gm65Scanner::with_default_config(uart);

    defmt::info!("Running HIL tests (sync)...");
    let results = hil_tests::run_hil_tests(&mut scanner);

    if results.all_passed() {
        defmt::info!("All HIL tests passed!");

        defmt::info!("========================================");
        defmt::info!("QR Scan Test");
        defmt::info!("========================================");
        defmt::info!("Present a QR code to the scanner now...");
        defmt::info!("You have 5 seconds. Aim laser is ON.");

        let aim_settings =
            ScannerSettings::ALWAYS_ON | ScannerSettings::COMMAND | ScannerSettings::AIM;
        let _ = scanner.set_scanner_settings(aim_settings);

        let max_retries: u32 = 50;
        let mut qr_result = false;
        for i in 0..max_retries {
            let _ = scanner.trigger_scan();
            if let Some(_data) = scanner.read_scan() {
                defmt::info!("QR SCAN TEST PASSED!");
                qr_result = true;
                break;
            }
            if i < max_retries - 1 {
                delay(100 * 180_000);
            }
        }

        if !qr_result {
            defmt::error!("QR SCAN TEST FAILED");
        }

        let _ = scanner.set_scanner_settings(ScannerSettings::default());
    } else {
        defmt::error!("HIL tests failed: {}/5", results.passed_count());
        defmt::info!("Skipping QR scan test.");
    }

    loop {}
}
