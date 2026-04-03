#![no_std]
#![no_main]
#![allow(dead_code, clippy::empty_loop)]

extern crate alloc;

use cortex_m_rt::entry;

#[cfg(feature = "defmt")]
use defmt_rtt as _;
#[cfg(not(feature = "defmt"))]
use panic_halt as _;
#[cfg(feature = "defmt")]
use panic_probe as _;

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565, prelude::*};
use static_cell::ConstStaticCell;
use stm32f469i_disc::{
    hal,
    hal::ltdc::{Layer, LtdcFramebuffer},
    hal::pac::{self, CorePeripherals},
    hal::prelude::*,
    hal::rcc,
    hal::serial::Serial6,
    lcd, sdram,
    sdram::alt,
    usb,
};

use hal::otg_fs::{UsbBus, UsbBusType};
use usb_device::prelude::*;

use gm65_scanner::{Gm65Scanner, ScannerDriverSync, ScannerModel, ScannerSettings, ScannerState};

mod cdc;
mod display_utils;
mod display {
    const DISPLAY_CENTER_X: i32 = 400;
    const DISPLAY_MAX_Y: u32 = 480;
    include!("display.rs");
}
mod qr_display {
    include!("qr_display.rs");
}

use cdc::{CdcPort, Command, Response, Status, MAX_PAYLOAD_SIZE};
use display::render_decoded_scan;

static EP_MEMORY: ConstStaticCell<[u32; 1024]> = ConstStaticCell::new([0; 1024]);

fn render_boot_status(fb: &mut impl DrawTarget<Color = Rgb565>, line: &str, line_num: u32) {
    use embedded_graphics::mono_font::{ascii::FONT_6X10, MonoTextStyle};
    use embedded_graphics::text::{Alignment, Text, TextStyleBuilder};
    let style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
    let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(
        line,
        Point::new(240, (20 + line_num * 14) as i32),
        style,
        center,
    )
    .draw(fb)
    .ok();
}

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

#[entry]
fn main() -> ! {
    let dp = pac::Peripherals::take().unwrap();
    let cp = CorePeripherals::take().unwrap();

    let mut rcc = dp.RCC.freeze(
        rcc::Config::hse(8.MHz())
            .pclk2(32.MHz())
            .sysclk(180.MHz())
            .require_pll48clk(),
    );
    let mut delay = cp.SYST.delay(&rcc.clocks);

    let gpioa = dp.GPIOA.split(&mut rcc);
    let gpioc = dp.GPIOC.split(&mut rcc);
    let gpiod = dp.GPIOD.split(&mut rcc);
    let gpioe = dp.GPIOE.split(&mut rcc);
    let gpiof = dp.GPIOF.split(&mut rcc);
    let gpiog = dp.GPIOG.split(&mut rcc);
    let scanner_tx = gpiog.pg14;
    let scanner_rx = gpiog.pg9;
    let gpioh = dp.GPIOH.split(&mut rcc);
    let gpioi = dp.GPIOI.split(&mut rcc);

    let mut lcd_reset = gpioh.ph7.into_push_pull_output();
    lcd_reset.set_low();
    delay.delay_ms(20u32);
    lcd_reset.set_high();
    delay.delay_ms(10u32);

    let sdram = sdram::Sdram::new(
        dp.FMC,
        sdram::sdram_pins!(gpioc, gpiod, gpioe, gpiof, gpiog, gpioh, gpioi),
        &rcc.clocks,
        &mut delay,
    );

    {
        const HEAP_SIZE: usize = 64 * 1024;
        let heap_start = sdram.mem as *mut u8;
        let fb_bytes = lcd::DisplayOrientation::Portrait.fb_size() * core::mem::size_of::<u16>();
        unsafe {
            let heap_ptr = heap_start.add(fb_bytes);
            ALLOCATOR.lock().init(heap_ptr, HEAP_SIZE);
        }
    }

    let orientation = lcd::DisplayOrientation::Portrait;
    let fb_buffer: &'static mut [u16] = unsafe {
        &mut *core::ptr::slice_from_raw_parts_mut(sdram.mem as *mut u16, orientation.fb_size())
    };
    let mut fb = LtdcFramebuffer::new(fb_buffer, orientation.width(), orientation.height());

    let (mut display_ctrl, _controller, _orient) = lcd::init_display_full(
        dp.DSI,
        dp.LTDC,
        dp.DMA2D,
        &mut rcc,
        &mut delay,
        lcd::BoardHint::ForceNt35510,
        orientation,
    );

    fb.clear(Rgb565::CSS_BLACK).ok();

    let fb_buffer = fb.into_inner();
    display_ctrl.config_layer(Layer::L1, fb_buffer, hal::ltdc::PixelFormat::RGB565);
    display_ctrl.enable_layer(Layer::L1);
    display_ctrl.reload();

    let fb_ptr = display_ctrl
        .layer_buffer_mut(Layer::L1)
        .expect("layer L1 buffer");
    let fb_buf: &'static mut [u16] = unsafe { core::mem::transmute(fb_ptr) };
    let mut fb = LtdcFramebuffer::new(fb_buf, orientation.width(), orientation.height());

    let mut boot_line: u32 = 0;
    render_boot_status(&mut fb, "[OK] Display", boot_line);
    boot_line += 1;
    render_boot_status(&mut fb, "[..] USB...", boot_line);
    boot_line += 1;

    let usb_periph = usb::init(
        (dp.OTG_FS_GLOBAL, dp.OTG_FS_DEVICE, dp.OTG_FS_PWRCLK),
        gpioa.pa11,
        gpioa.pa12,
        &rcc.clocks,
    );

    let usb_bus = UsbBus::new(usb_periph, EP_MEMORY.take());

    let serial: usbd_serial::SerialPort<'static, UsbBusType> =
        unsafe { core::mem::transmute(usbd_serial::SerialPort::new(&usb_bus)) };

    // USB CDC ACM device setup.
    //
    // Per USB Device Class Definition for Communications Devices 1.2:
    // - Class 0x02 (Communications Device Class)
    // - SubClass 0x02 (Abstract Control Model)
    // - Protocol 0x01 (AT Commands / Common AT)
    //
    // VID 0x16C0 (Van Ooijen Technische Informatica / VOTI) with PID 0x27DD
    // is a shared test VID/PID for CDC ACM devices. For production use,
    // obtain a unique VID from USB-IF or use pid.codes (https://pid.codes/).
    //
    // USB 2.0 Specification §9.6.1 defines the device descriptor format.
    // String descriptors (manufacturer, product, serial) per §9.6.7.
    let mut usb_dev: UsbDevice<'static, UsbBusType> = unsafe {
        core::mem::transmute(
            UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
                .device_class(usbd_serial::USB_CLASS_CDC)
                .strings(&[StringDescriptors::default()
                    .manufacturer("gm65-scanner")
                    .product("QR Barcode Scanner")
                    .serial_number("F4691")])
                .unwrap()
                .build(),
        )
    };

    let mut cdc_port = CdcPort::new(serial);

    render_boot_status(&mut fb, "[OK] USB", boot_line - 1);
    render_boot_status(&mut fb, "[..] Scanner...", boot_line);
    boot_line += 1;

    let baud = 115200;
    let uart = dp
        .USART6
        .serial((scanner_tx, scanner_rx), baud.bps(), &mut rcc)
        .unwrap();
    let mut scanner = Gm65Scanner::with_default_config(uart);

    let mut model_str: &str = "Unknown";
    let scanner_connected = match scanner.init() {
        Ok(model) => {
            model_str = match model {
                ScannerModel::Gm65 => "GM65",
                ScannerModel::M3Y => "M3Y",
                ScannerModel::Generic => "Generic",
                ScannerModel::Unknown => "Unknown",
            };
            true
        }
        Err(_) => false,
    };

    if scanner_connected {
        render_boot_status(&mut fb, "[OK] Scanner", boot_line - 1);
    } else {
        render_boot_status(&mut fb, "[!!] Scanner FAIL", boot_line - 1);
    }
    boot_line += 1;
    render_boot_status(&mut fb, "[OK] Ready", boot_line);

    if scanner_connected {
        if let Some(settings) = scanner.get_scanner_settings() {
            display::render_scanner_settings(&mut fb, settings);
        } else {
            display::render_home(&mut fb, true, model_str);
        }
    } else {
        display::render_home(&mut fb, false, model_str);
    }

    let mut last_scan_data: Option<[u8; MAX_PAYLOAD_SIZE - 1]> = None;
    let mut last_scan_len: usize = 0;
    let mut auto_scan: bool = scanner_connected;

    loop {
        if usb_dev.poll(&mut [cdc_port.serial_mut()]) {
            if let Some(frame) = cdc_port.receive_frame() {
                if frame.command == Command::SetSettings || frame.command == Command::ScannerTrigger
                {
                    auto_scan = false;
                }
                let was_auto = auto_scan;
                auto_scan = false;
                let response = handle_command(
                    frame.command,
                    frame.payload(),
                    &mut fb,
                    &mut scanner,
                    &mut last_scan_data,
                    &mut last_scan_len,
                );
                cdc_port.send_response(&response);
                if was_auto {
                    auto_scan = true;
                }
            }
        }

        if auto_scan && !scanner.data_ready() && scanner.state() == ScannerState::Ready {
            let _ = scanner.trigger_scan();
        }

        if !scanner.data_ready() {
            for _ in 0..8 {
                if let Some(data) = scanner.try_read_scan() {
                    let payload = gm65_scanner::decode_payload(&data);
                    render_decoded_scan(&mut fb, &payload);
                    if data.len() <= 200 && core::str::from_utf8(&data).is_ok() {
                        qr_display::render_qr_mirror(&mut fb, &data);
                    }
                    let copy_len = data.len().min(MAX_PAYLOAD_SIZE - 1);
                    let mut buf = [0u8; MAX_PAYLOAD_SIZE - 1];
                    buf[..copy_len].copy_from_slice(&data[..copy_len]);
                    last_scan_data = Some(buf);
                    last_scan_len = copy_len;
                    break;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_command(
    command: Command,
    payload: &[u8],
    fb: &mut LtdcFramebuffer<u16>,
    scanner: &mut Gm65Scanner<Serial6>,
    last_scan_data: &mut Option<[u8; MAX_PAYLOAD_SIZE - 1]>,
    last_scan_len: &mut usize,
) -> Response {
    match command {
        Command::ScannerStatus => handle_scanner_status(scanner),
        Command::ScannerTrigger => handle_scanner_trigger(scanner, fb),
        Command::ScannerData => handle_scanner_data(fb, last_scan_data, last_scan_len),
        Command::GetSettings => handle_get_settings(scanner, fb),
        Command::SetSettings => handle_set_settings(scanner, payload, fb),
        Command::DisplayQr => handle_display_qr(payload, fb),
        Command::EnterSettings => Response::new(Status::Ok),
        Command::GetCompatibilityProfile
        | Command::SetCompatibilityProfile
        | Command::RebootUsb
        | Command::GetHostOptions
        | Command::SetHostOptions => {
            // The sync firmware is intentionally the legacy/reference CDC image.
            // DS2208 profile management lives in the async firmware only.
            Response::new(Status::InvalidCommand)
        }
    }
}

fn handle_scanner_status(scanner: &mut Gm65Scanner<Serial6>) -> Response {
    let _ = scanner.stop_scan();
    let status = scanner.status();
    let mut payload = [0u8; MAX_PAYLOAD_SIZE];
    let mut offset = 0;

    payload[offset] = if status.connected { 1 } else { 0 };
    offset += 1;
    payload[offset] = if status.initialized { 1 } else { 0 };
    offset += 1;

    let model_byte: u8 = match status.model {
        ScannerModel::Gm65 => 0x01,
        ScannerModel::M3Y => 0x02,
        ScannerModel::Generic => 0x03,
        ScannerModel::Unknown => 0x00,
    };
    payload[offset] = model_byte;
    offset += 1;

    Response::with_payload(Status::Ok, &payload[..offset])
        .unwrap_or_else(|| Response::new(Status::Error))
}

fn handle_scanner_trigger(
    scanner: &mut Gm65Scanner<Serial6>,
    fb: &mut LtdcFramebuffer<u16>,
) -> Response {
    match scanner.trigger_scan() {
        Ok(()) => {
            display::render_status(fb, "Scanning...");
            Response::new(Status::Ok)
        }
        Err(_) => {
            display::render_error(fb, "Scanner error");
            Response::new(Status::ScannerNotConnected)
        }
    }
}

fn handle_scanner_data(
    fb: &mut LtdcFramebuffer<u16>,
    last_scan_data: &mut Option<[u8; MAX_PAYLOAD_SIZE - 1]>,
    last_scan_len: &mut usize,
) -> Response {
    match last_scan_data.take() {
        Some(data) => {
            let len = *last_scan_len;
            let payload_type = gm65_scanner::classify_payload(&data[..len]);
            let payload = gm65_scanner::decode_payload(&data[..len]);
            render_decoded_scan(fb, &payload);

            let type_byte: u8 = match payload_type {
                gm65_scanner::PayloadType::CashuV4 => 0x01,
                gm65_scanner::PayloadType::CashuV3 => 0x02,
                gm65_scanner::PayloadType::UrFragment => 0x03,
                gm65_scanner::PayloadType::PlainText | gm65_scanner::PayloadType::Url => 0x00,
                gm65_scanner::PayloadType::Binary => 0x04,
            };
            let mut buf = [0u8; MAX_PAYLOAD_SIZE];
            buf[0] = type_byte;
            buf[1..len + 1].copy_from_slice(&data[..len]);
            Response::with_payload(Status::Ok, &buf[..len + 1])
                .unwrap_or_else(|| Response::new(Status::BufferOverflow))
        }
        None => Response::new(Status::NoScanData),
    }
}

fn handle_get_settings(
    scanner: &mut Gm65Scanner<Serial6>,
    fb: &mut LtdcFramebuffer<u16>,
) -> Response {
    let _ = scanner.stop_scan();
    match scanner.get_scanner_settings() {
        Some(settings) => {
            display::render_scanner_settings(fb, settings);
            Response::with_payload(Status::Ok, &[settings.bits()])
                .unwrap_or_else(|| Response::new(Status::Error))
        }
        None => {
            display::render_error(fb, "Failed to read settings");
            Response::new(Status::Error)
        }
    }
}

fn handle_set_settings(
    scanner: &mut Gm65Scanner<Serial6>,
    payload: &[u8],
    fb: &mut LtdcFramebuffer<u16>,
) -> Response {
    let _ = scanner.stop_scan();
    if payload.is_empty() {
        return Response::new(Status::InvalidPayload);
    }
    let raw = payload[0];
    match ScannerSettings::from_bits(raw) {
        Some(settings) => {
            if scanner.set_scanner_settings(settings) {
                if let Some(readback) = scanner.get_scanner_settings() {
                    display::render_scanner_settings(fb, readback);
                    Response::with_payload(Status::Ok, &[readback.bits()])
                        .unwrap_or_else(|| Response::new(Status::Error))
                } else {
                    display::render_scanner_settings(fb, settings);
                    Response::with_payload(Status::Ok, &[raw])
                        .unwrap_or_else(|| Response::new(Status::Error))
                }
            } else {
                display::render_error(fb, "Set failed");
                Response::new(Status::Error)
            }
        }
        None => Response::new(Status::InvalidPayload),
    }
}

fn handle_display_qr(payload: &[u8], fb: &mut LtdcFramebuffer<u16>) -> Response {
    let text = core::str::from_utf8(payload).unwrap_or("<invalid utf8>");
    if qr_display::render_qr_code(fb, text) {
        Response::new(Status::Ok)
    } else {
        display::render_error(fb, "QR encode failed");
        Response::new(Status::Error)
    }
}
