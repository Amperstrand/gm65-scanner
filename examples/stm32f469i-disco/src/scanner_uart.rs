use core::cell::RefCell;
use heapless::spsc::Queue;

use stm32f469i_disc::hal::pac::USART6;
use stm32f469i_disc::hal::serial::Tx;

pub struct ScannerUart {
    tx: Tx<USART6>,
}

impl ScannerUart {
    pub fn new(tx: Tx<USART6>) -> Self {
        Self { tx }
    }
}

impl embedded_hal_02::serial::Write<u8> for ScannerUart {
    type Error = ();

    fn write(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
        self.tx.write(byte).map_err(|e| match e {
            nb::Error::WouldBlock => nb::Error::WouldBlock,
            nb::Error::Other(_) => nb::Error::Other(()),
        })
    }

    fn flush(&mut self) -> nb::Result<(), Self::Error> {
        self.tx.flush().map_err(|e| match e {
            nb::Error::WouldBlock => nb::Error::WouldBlock,
            nb::Error::Other(_) => nb::Error::Other(()),
        })
    }
}

impl embedded_hal_02::serial::Read<u8> for ScannerUart {
    type Error = ();

    fn read(&mut self) -> nb::Result<u8, Self::Error> {
        cortex_m::interrupt::free(|cs| {
            RING.borrow(cs).borrow_mut().dequeue().ok_or(nb::Error::WouldBlock)
        })
    }
}

const RING_SIZE: usize = 512;

static RING: cortex_m::interrupt::Mutex<RefCell<Queue<u8, RING_SIZE>>> =
    cortex_m::interrupt::Mutex::new(RefCell::new(Queue::new()));

static ISR_BYTES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static ISR_ORE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static ISR_FIRES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

pub fn init_scanner_uart(
    serial: stm32f469i_disc::hal::serial::Serial<USART6>,
) -> ScannerUart {
    let (tx, _rx) = serial.split();

    let usart = unsafe { &*USART6::ptr() };
    usart.cr1().modify(|_, w| w.rxneie().set_bit());
    unsafe { cortex_m::peripheral::NVIC::unmask(stm32f469i_disc::hal::pac::Interrupt::USART6) };

    ScannerUart::new(tx)
}

pub fn handle_usart6_interrupt() {
    ISR_FIRES.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    let usart = unsafe { &*USART6::ptr() };
    let sr = usart.sr().read();

    if sr.rxne().bit_is_set() {
        let byte = usart.dr().read().bits() as u8;
        ISR_BYTES.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        cortex_m::interrupt::free(|cs| {
            RING.borrow(cs).borrow_mut().enqueue(byte).ok();
        });
    }

    if sr.ore().bit_is_set() {
        ISR_ORE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let _ = usart.dr().read();
    }
}

pub fn ring_stats() -> (u32, u32, u32, usize) {
    let bytes = ISR_BYTES.load(core::sync::atomic::Ordering::Relaxed);
    let ore = ISR_ORE.load(core::sync::atomic::Ordering::Relaxed);
    let fires = ISR_FIRES.load(core::sync::atomic::Ordering::Relaxed);
    let len = cortex_m::interrupt::free(|cs| RING.borrow(cs).borrow().len());
    (bytes, ore, fires, len)
}
