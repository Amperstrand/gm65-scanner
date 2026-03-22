//! HIL Test Binary for Sync Driver
//!
//! Run with: cargo run --target thumbv7em-none-eabihf --bin hil_test_sync

#![no_std]
#![no_main]

extern crate alloc;

use cortex_m_rt::entry;
use defmt_rtt as _;
use panic_probe as _;

use gm65_scanner::{driver::hil_tests, Gm65Scanner};
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
    let tx = gpiog.pg14;
    let rx = gpiog.pg9;

    let serial = Serial6::new(dp.USART6, (tx, rx), 9600.bps(), &mut rcc).unwrap();
    let mut scanner = Gm65Scanner::with_default_config(serial);

    defmt::info!("Running HIL tests (sync)...");
    let results = hil_tests::run_hil_tests(&mut scanner);

    if results.all_passed() {
        defmt::info!("All HIL tests passed!");
    } else {
        defmt::error!("HIL tests failed: {}/5", results.passed_count());
    }

    loop {}
}
