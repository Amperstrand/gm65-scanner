#![no_main]
#![no_std]

use cortex_m_rt::entry;
use panic_halt as _;
use static_cell::ConstStaticCell;

use stm32f469i_disc::hal::{
    otg_fs::{UsbBus, USB},
    pac,
    prelude::*,
    rcc,
};
use usb_device::prelude::*;

static EP_MEMORY: ConstStaticCell<[u32; 1024]> = ConstStaticCell::new([0; 1024]);

#[entry]
fn main() -> ! {
    let dp = pac::Peripherals::take().unwrap();
    let mut rcc = dp.RCC.freeze(
        rcc::Config::hse(8.MHz())
            .sysclk(180.MHz())
            .require_pll48clk(),
    );
    let gpioa = dp.GPIOA.split(&mut rcc);

    let usb = USB::new(
        (dp.OTG_FS_GLOBAL, dp.OTG_FS_DEVICE, dp.OTG_FS_PWRCLK),
        (gpioa.pa11, gpioa.pa12),
        &rcc.clocks,
    );
    let usb_bus = UsbBus::new(usb, EP_MEMORY.take());
    let mut serial = usbd_serial::SerialPort::new(&usb_bus);
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .device_class(usbd_serial::USB_CLASS_CDC)
        .strings(&[StringDescriptors::default()
            .manufacturer("gm65-test")
            .product("USB minimal")
            .serial_number("T1")])
        .unwrap()
        .build();

    let mut rx_buf = [0u8; 128];
    loop {
        if !usb_dev.poll(&mut [&mut serial]) {
            continue;
        }
        match serial.read(&mut rx_buf) {
            Ok(count) if count > 0 => {
                let _ = serial.write(&rx_buf[..count]);
            }
            _ => {}
        }
    }
}
