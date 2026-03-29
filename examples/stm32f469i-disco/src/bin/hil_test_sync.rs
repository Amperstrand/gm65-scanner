//! HIL Test Binary for Sync Driver
//!
//! LED feedback:
//!   Green  (LD1, PG6) — scanner detected, test pass, QR success
//!   Orange (LD2, PD4) — test running, QR scan waiting
//!   Red    (LD3, PD5) — scanner not detected, test fail
//!   Blue   (LD4, PK3) — all tests passed
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
use stm32f469i_disc::{hal::pac, hal::prelude::*, hal::rcc, hal::serial::Serial6, led::Led};

const HEAP_SIZE: usize = 32 * 1024;
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

fn blink(led: &mut Led, cycles: u32, on_cycles: u32, off_cycles: u32) {
    for _ in 0..cycles {
        led.on();
        delay(on_cycles);
        led.off();
        delay(off_cycles);
    }
}

fn all_off(leds: &mut [&mut Led; 4]) {
    for led in leds.iter_mut() {
        led.off();
    }
}

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
    let gpiod = dp.GPIOD.split(&mut rcc);
    let gpiok = dp.GPIOK.split(&mut rcc);

    let scanner_tx = gpiog.pg14;
    let scanner_rx = gpiog.pg9;

    let mut led_green: Led = gpiog.pg6.into_push_pull_output().into();
    let mut led_orange: Led = gpiod.pd4.into_push_pull_output().into();
    let mut led_red: Led = gpiod.pd5.into_push_pull_output().into();
    let mut led_blue: Led = gpiok.pk3.into_push_pull_output().into();

    let mut leds: [&mut Led; 4] = [&mut led_green, &mut led_orange, &mut led_red, &mut led_blue];
    all_off(&mut leds);

    let uart = Serial6::new(dp.USART6, (scanner_tx, scanner_rx), 115200.bps(), &mut rcc).unwrap();
    let mut scanner = Gm65Scanner::with_default_config(uart);

    defmt::info!("========================================");
    defmt::info!("HIL Tests (sync driver)");
    defmt::info!("========================================");

    blink(&mut led_green, 2, 10 * 180_000, 10 * 180_000);
    defmt::info!("Running 5 core HIL tests...");
    led_orange.on();

    let results = hil_tests::run_hil_tests(&mut scanner);

    led_orange.off();

    if results.init_detects_scanner {
        defmt::info!("[1/5] init_detects_scanner: PASS");
        blink(&mut led_green, 1, 5 * 180_000, 5 * 180_000);
    } else {
        defmt::error!("[1/5] init_detects_scanner: FAIL");
        led_red.on();
        loop {}
    }

    if results.ping_after_init {
        defmt::info!("[2/5] ping_after_init: PASS");
        blink(&mut led_green, 1, 5 * 180_000, 5 * 180_000);
    } else {
        defmt::error!("[2/5] ping_after_init: FAIL");
        led_red.on();
        loop {}
    }

    if results.trigger_and_stop {
        defmt::info!("[3/5] trigger_and_stop: PASS");
        blink(&mut led_green, 1, 5 * 180_000, 5 * 180_000);
    } else {
        defmt::error!("[3/5] trigger_and_stop: FAIL");
        led_red.on();
        loop {}
    }

    if results.read_scan_timeout {
        defmt::info!("[4/5] read_scan_timeout: PASS");
        blink(&mut led_green, 1, 5 * 180_000, 5 * 180_000);
    } else {
        defmt::error!("[4/5] read_scan_timeout: FAIL");
        led_red.on();
        loop {}
    }

    if results.state_transitions {
        defmt::info!("[5/5] state_transitions: PASS");
        blink(&mut led_green, 1, 5 * 180_000, 5 * 180_000);
    } else {
        defmt::error!("[5/5] state_transitions: FAIL");
        led_red.on();
        loop {}
    }

    defmt::info!("All 5 core HIL tests passed!");
    led_green.on();

    defmt::info!("========================================");
    defmt::info!("QR Scan Test");
    defmt::info!("========================================");
    defmt::info!("Present a QR code to the scanner now...");
    defmt::info!("You have 5 seconds. Aim laser is ON.");
    defmt::info!("Orange LED blinks while waiting for scan.");

    let aim_settings = ScannerSettings::ALWAYS_ON | ScannerSettings::COMMAND | ScannerSettings::AIM;
    let _ = scanner.set_scanner_settings(aim_settings);

    let max_retries: u32 = 50;
    let mut qr_result = false;
    led_green.off();

    for i in 0..max_retries {
        led_orange.on();
        let _ = scanner.trigger_scan();
        if let Some(data) = scanner.read_scan() {
            let len = data.len();
            led_orange.off();
            defmt::info!("QR SCAN TEST PASSED! ({} bytes)", len);
            qr_result = true;
            break;
        }
        led_orange.off();
        if i < max_retries - 1 {
            delay(100 * 180_000);
        }
    }

    let _ = scanner.set_scanner_settings(ScannerSettings::default());

    if qr_result {
        blink(&mut led_green, 3, 10 * 180_000, 10 * 180_000);
        defmt::info!("========================================");
        defmt::info!("ALL TESTS PASSED");
        defmt::info!("========================================");
        led_green.on();
        led_orange.on();
        led_blue.on();
    } else {
        defmt::error!("QR SCAN TEST FAILED");
        blink(&mut led_red, 1, 50 * 180_000, 50 * 180_000);
    }

    loop {}
}
