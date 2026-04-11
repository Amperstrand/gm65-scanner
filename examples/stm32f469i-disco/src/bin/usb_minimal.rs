#![no_std]
#![no_main]

use panic_halt as _;

use embassy_executor::Spawner;
use embassy_stm32::time::Hertz;
use embassy_stm32::{bind_interrupts, peripherals, rcc::*, usb, Config};
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::Builder;

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();
static mut HEAP_MEMORY: [u8; 4096] = [0; 4096];

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    unsafe {
        ALLOCATOR
            .lock()
            .init(core::ptr::addr_of_mut!(HEAP_MEMORY) as *mut u8, 4096);
    }

    let mut config = Config::default();
    config.rcc.hse = Some(Hse {
        freq: Hertz(8_000_000),
        mode: HseMode::Oscillator,
    });
    config.rcc.pll_src = PllSource::HSE;
    config.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV8,
        mul: PllMul::MUL360,
        divp: Some(PllPDiv::DIV2),
        divq: Some(PllQDiv::DIV7),
        divr: Some(PllRDiv::DIV6),
    });
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV4;
    config.rcc.apb2_pre = APBPrescaler::DIV2;
    config.rcc.sys = Sysclk::PLL1_P;
    config.rcc.mux.clk48sel = mux::Clk48sel::PLLSAI1_Q;
    config.rcc.pllsai = Some(Pll {
        prediv: PllPreDiv::DIV8,
        mul: PllMul::MUL384,
        divp: Some(PllPDiv::DIV8),
        divq: Some(PllQDiv::DIV8),
        divr: Some(PllRDiv::DIV7),
    });

    let p = embassy_stm32::init(config);

    stm32_metapac::RCC.dckcfgr2().modify(|w| {
        w.set_clk48sel(mux::Clk48sel::PLLSAI1_Q);
    });

    let mut ep_out_buffer = [0u8; 256];
    let mut usb_config = usb::Config::default();
    usb_config.vbus_detection = false;
    let usb_driver = usb::Driver::new_fs(
        p.USB_OTG_FS,
        Irqs,
        p.PA12,
        p.PA11,
        &mut ep_out_buffer,
        usb_config,
    );

    let mut usb_config_desc = embassy_usb::Config::new(0xc0de, 0xcafe);
    usb_config_desc.manufacturer = Some("gm65-scanner");
    usb_config_desc.product = Some("Minimal USB Test");
    usb_config_desc.serial_number = Some("minimal");

    let mut config_descriptor = [0u8; 256];
    let mut bos_descriptor = [0u8; 256];
    let mut msos_descriptor = [0u8; 256];
    let mut control_buf = [0u8; 64];

    let mut usb_state = State::new();
    let mut usb_builder = Builder::new(
        usb_driver,
        usb_config_desc,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );
    let mut cdc = CdcAcmClass::new(&mut usb_builder, &mut usb_state, 64);
    let mut usb_dev = usb_builder.build();

    let usb_task = async {
        usb_dev.run().await;
    };

    let blink_task = async {
        let mut led = embassy_stm32::gpio::Output::new(
            unsafe { embassy_stm32::peripherals::PG6::steal() },
            embassy_stm32::gpio::Level::Low,
            embassy_stm32::gpio::Speed::Low,
        );
        loop {
            led.set_high();
            Timer::after(embassy_time::Duration::from_millis(500)).await;
            led.set_low();
            Timer::after(embassy_time::Duration::from_millis(500)).await;
        }
    };

    let cdc_task = async {
        loop {
            cdc.wait_connection().await;
            let mut buf = [0u8; 64];
            loop {
                match cdc.read_packet(&mut buf).await {
                    Ok(n) if n > 0 => {
                        let _ = cdc.write_packet(&buf[..n]).await;
                    }
                    Ok(_) => {
                        Timer::after(embassy_time::Duration::from_secs(5)).await;
                        if cdc.write_packet(b"PONG\n").await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            Timer::after(embassy_time::Duration::from_millis(100)).await;
        }
    };

    embassy_futures::join::join3(usb_task, blink_task, cdc_task).await;
}
