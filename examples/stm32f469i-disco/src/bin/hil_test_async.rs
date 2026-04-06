//! Async HIL Test Binary for Async Driver
//!
//! Uses embassy executor + AsyncUart wrapper around embassy-stm32 blocking UART.
//!
//! LED feedback:
//!   Green  (LD1, PG6) — scanner detected, test pass, QR success
//!   Orange (LD2, PD4) — test running, QR scan waiting
//!   Red    (LD3, PD5) — scanner not detected, test fail
//!   Blue   (LD4, PK3) — extended tests running, all pass
//!
//! Run: cargo run --release --target thumbv7em-none-eabihf --bin hil_test_async --features scanner-async,defmt

#![no_std]
#![no_main]

extern crate alloc;

#[cfg(feature = "scanner-async")]
use defmt_rtt as _;
#[cfg(feature = "scanner-async")]
use panic_probe as _;

#[cfg(feature = "scanner-async")]
use embassy_executor::Spawner;
#[cfg(feature = "scanner-async")]
use embassy_stm32::{
    gpio::{Level, Output, Speed},
    interrupt::InterruptExt,
    usart, Config,
};
#[cfg(feature = "scanner-async")]
use embassy_time::Timer;
#[cfg(feature = "scanner-async")]
use gm65_scanner::{
    driver::{async_hil_tests as hil_tests, run_extended_hil_tests},
    Gm65ScannerAsync, ScannerSettings,
};
#[cfg(feature = "scanner-async")]
use linked_list_allocator::LockedHeap;

mod async_shared {
    #[cfg(feature = "scanner-async")]
    include!("../async_shared.rs");
}

#[cfg(feature = "scanner-async")]
const HEAP_SIZE: usize = 32 * 1024;
#[cfg(feature = "scanner-async")]
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();
#[cfg(feature = "scanner-async")]
static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[cfg(feature = "scanner-async")]
async fn blink(led: &mut Output<'_>, count: u32, on_ms: u64, off_ms: u64) {
    for _ in 0..count {
        led.set_high();
        Timer::after_millis(on_ms).await;
        led.set_low();
        Timer::after_millis(off_ms).await;
    }
}

#[cfg(feature = "scanner-async")]
#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    unsafe {
        ALLOCATOR
            .lock()
            .init(core::ptr::addr_of_mut!(HEAP_MEMORY) as *mut u8, HEAP_SIZE);
    }

    defmt::info!("========================================");
    defmt::info!("Async HIL Tests (embassy executor)");
    defmt::info!("========================================");

    let p = embassy_stm32::init(Config::default());

    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;

    let uart = usart::Uart::new_blocking(p.USART6, p.PG9, p.PG14, uart_config).unwrap();
    embassy_stm32::interrupt::USART6.disable();

    let async_uart = async_shared::AsyncUart {
        inner: uart,
    };
    let mut scanner = Gm65ScannerAsync::with_default_config(async_uart);

    let mut led_green = Output::new(p.PG6, Level::Low, Speed::Low);
    let mut led_orange = Output::new(p.PD4, Level::Low, Speed::Low);
    let mut led_red = Output::new(p.PD5, Level::Low, Speed::Low);
    let mut led_blue = Output::new(p.PK3, Level::Low, Speed::Low);

    blink(&mut led_green, 2, 200, 200).await;

    defmt::info!("Running 5 core HIL tests...");
    led_orange.set_high();

    let results = hil_tests::run_hil_tests(&mut scanner).await;

    led_orange.set_low();

    if results.init_detects_scanner {
        defmt::info!("[1/5] init_detects_scanner: PASS");
        blink(&mut led_green, 1, 100, 100).await;
    } else {
        defmt::error!("[1/5] init_detects_scanner: FAIL");
        led_red.set_high();
        loop {}
    }

    if results.ping_after_init {
        defmt::info!("[2/5] ping_after_init: PASS");
        blink(&mut led_green, 1, 100, 100).await;
    } else {
        defmt::error!("[2/5] ping_after_init: FAIL");
        led_red.set_high();
        loop {}
    }

    if results.trigger_and_stop {
        defmt::info!("[3/5] trigger_and_stop: PASS");
        blink(&mut led_green, 1, 100, 100).await;
    } else {
        defmt::error!("[3/5] trigger_and_stop: FAIL");
        led_red.set_high();
        loop {}
    }

    if results.read_scan_timeout {
        defmt::info!("[4/5] read_scan_timeout: PASS");
        blink(&mut led_green, 1, 100, 100).await;
    } else {
        defmt::error!("[4/5] read_scan_timeout: FAIL");
        led_red.set_high();
        loop {}
    }

    if results.state_transitions {
        defmt::info!("[5/5] state_transitions: PASS");
        blink(&mut led_green, 1, 100, 100).await;
    } else {
        defmt::error!("[5/5] state_transitions: FAIL");
        led_red.set_high();
        loop {}
    }

    defmt::info!("All 5 core HIL tests passed!");
    led_green.set_high();

    defmt::info!("========================================");
    defmt::info!("Extended HIL Tests");
    defmt::info!("========================================");
    defmt::info!("Running 3 extended tests...");
    led_green.set_low();
    led_blue.set_high();

    let ext_pass = run_extended_hil_tests(&mut scanner).await;

    led_blue.set_low();

    if ext_pass {
        defmt::info!("All 3 extended tests passed!");
        blink(&mut led_blue, 2, 100, 100).await;
    } else {
        defmt::error!("Extended HIL tests FAILED");
        led_red.set_high();
        loop {}
    }

    defmt::info!("All 8 HIL tests passed!");
    led_green.set_high();

    defmt::info!("========================================");
    defmt::info!("QR Scan Test");
    defmt::info!("========================================");
    defmt::info!("Aim laser is ON. Point scanner at QR code.");
    defmt::info!("Orange LED blinks while waiting.");
    defmt::info!("You have 10 seconds.");

    let aim_settings = ScannerSettings::ALWAYS_ON | ScannerSettings::COMMAND | ScannerSettings::AIM;
    if scanner.set_scanner_settings(aim_settings).await {
        defmt::info!("Aim laser enabled - point at QR code now!");
    } else {
        defmt::warn!("Failed to enable aim laser");
    }

    let qr_result = {
        let blink_task = async {
            loop {
                led_orange.set_high();
                Timer::after_millis(100).await;
                led_orange.set_low();
                Timer::after_millis(100).await;
            }
        };
        let scan_task = hil_tests::run_hil_test_with_qr(&mut scanner);

        match embassy_futures::select::select(scan_task, blink_task).await {
            embassy_futures::select::Either::First(result) => result,
            embassy_futures::select::Either::Second(_) => unreachable!(),
        }
    };

    let _ = scanner
        .set_scanner_settings(ScannerSettings::default())
        .await;

    if qr_result {
        defmt::info!("QR SCAN TEST PASSED!");
        blink(&mut led_green, 3, 100, 100).await;
        defmt::info!("========================================");
        defmt::info!("ALL 9 TESTS PASSED");
        defmt::info!("========================================");
        led_green.set_high();
        led_orange.set_high();
        led_blue.set_high();
    } else {
        defmt::error!("QR SCAN TEST FAILED");
        blink(&mut led_red, 1, 500, 500).await;
    }

    defmt::info!("Done. Looping forever.");
    loop {}
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
