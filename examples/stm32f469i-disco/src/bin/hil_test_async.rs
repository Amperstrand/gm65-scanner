//! HIL Test Binary for Async Driver
//!
//! Run with: cargo run --target thumbv7em-none-eabihf --bin hil_test_async --features async-mode

#![no_std]
#![no_main]

#[cfg(feature = "async-mode")]
extern crate alloc;

#[cfg(feature = "async-mode")]
use defmt_rtt as _;

#[cfg(feature = "async-mode")]
use embassy_executor::Spawner;

#[cfg(feature = "async-mode")]
use embassy_stm32::{bind_interrupts, peripherals, usart, Config};

#[cfg(feature = "async-mode")]
use linked_list_allocator::LockedHeap;

#[cfg(not(feature = "async-mode"))]
use panic_probe as _;

#[cfg(feature = "async-mode")]
use panic_probe as _;

#[cfg(feature = "async-mode")]
use static_cell::StaticCell;

#[cfg(feature = "async-mode")]
use gm65_scanner::{driver::async_hil_tests, Gm65ScannerAsync};

#[cfg(feature = "async-mode")]
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[cfg(feature = "async-mode")]
bind_interrupts!(struct Irqs {
    USART6 => usart::BufferedInterruptHandler<peripherals::USART6>;
});

#[cfg(feature = "async-mode")]
#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Config::default());

    let mut config = usart::Config::default();
    config.baudrate = 9600;

    static TX_BUF: StaticCell<[u8; 256]> = StaticCell::new();
    static RX_BUF: StaticCell<[u8; 256]> = StaticCell::new();

    let uart = usart::BufferedUart::new(
        p.USART6,
        p.PC7,  // RX
        p.PC6,  // TX
        TX_BUF.init([0; 256]),
        RX_BUF.init([0; 256]),
        Irqs,
        config,
    )
    .unwrap();

    let mut scanner = Gm65ScannerAsync::with_default_config(uart);

    defmt::info!("Running HIL tests (async)...");
    let results = async_hil_tests::run_hil_tests(&mut scanner).await;

    if results.all_passed() {
        defmt::info!("All HIL tests passed!");
    } else {
        defmt::error!("HIL tests failed: {}/5", results.passed_count());
    }

    loop {}
}

#[cfg(not(feature = "async-mode"))]
use cortex_m_rt::entry;

#[cfg(not(feature = "async-mode"))]
use defmt_rtt as _;

#[cfg(not(feature = "async-mode"))]
#[entry]
fn main() -> ! {
    defmt::error!("This binary requires the 'async-mode' feature. Use: cargo build --features async-mode --bin hil_test_async");
    loop {
        cortex_m::asm::wfi();
    }
}
