//! Enhanced Async Scanner Firmware — embassy executor
//!
//! Features:
//! - Concurrent tasks: scanner, USB CDC, LED indicator, SDRAM, display, touch
//! - Bidirectional CDC protocol (7 commands: Status, Trigger, Data, GetSettings, SetSettings, DisplayQr, EnterSettings)
//! - Payload type classification (Cashu V4/V3, UR fragment, plain text, binary)
//! - Type-aware display rendering (scan results, settings, home screen, error screen)
//! - Touch-based settings UI (Sound, Aim, Light, Command toggles)
//! - Auto-scan mode with CDC-trigger override
//! - LED blink feedback on scan
//!
//! Run: cargo run --release --target thumbv7em-none-eabihf --bin async_firmware --no-default-features --features scanner-async,defmt

#![no_std]
#![no_main]
#![allow(clippy::let_unit_value)]

extern crate alloc;

#[cfg(feature = "scanner-async")]
use alloc::string::String;
#[cfg(feature = "scanner-async")]
use alloc::vec::Vec;

#[cfg(feature = "scanner-async")]
use defmt_rtt as _;
#[cfg(feature = "scanner-async")]
use panic_probe as _;

#[cfg(feature = "scanner-async")]
use embassy_executor::Spawner;
#[cfg(feature = "scanner-async")]
use embassy_stm32::time::Hertz;
#[cfg(feature = "scanner-async")]
use embassy_stm32::{i2c, interrupt::InterruptExt, peripherals, rcc::*, usart, usb, Config};
#[cfg(feature = "scanner-async")]
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
#[cfg(feature = "scanner-async")]
use embassy_sync::channel::Channel;
#[cfg(feature = "scanner-async")]
use embassy_sync::mutex::Mutex;
#[cfg(feature = "scanner-async")]
use embassy_sync::signal::Signal;
#[cfg(feature = "scanner-async")]
use embassy_time::{Duration, Ticker, Timer};
#[cfg(feature = "scanner-async")]
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
#[cfg(feature = "scanner-async")]
use embassy_usb::Builder;
#[cfg(feature = "scanner-async")]
use gm65_scanner::{Gm65ScannerAsync, ScannerDriver, ScannerModel, ScannerSettings};
#[cfg(feature = "scanner-async")]
use linked_list_allocator::LockedHeap;

#[cfg(feature = "scanner-async")]
use embassy_stm32f469i_disco::display::SdramCtrl;
#[cfg(feature = "scanner-async")]
use embassy_stm32f469i_disco::TouchCtrl;

mod async_shared {
    #[cfg(feature = "scanner-async")]
    include!("../async_shared.rs");
}

#[cfg(feature = "scanner-async")]
use embassy_stm32::bind_interrupts;

#[cfg(feature = "scanner-async")]
bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

#[cfg(feature = "scanner-async")]
const HEAP_SIZE: usize = 64 * 1024;
#[cfg(feature = "scanner-async")]
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();
#[cfg(feature = "scanner-async")]
static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[cfg(feature = "scanner-async")]
static SCAN_CHANNEL: Channel<CriticalSectionRawMutex, ScanResult, 4> = Channel::new();
#[cfg(feature = "scanner-async")]
static SDRAM_CHANNEL: Channel<CriticalSectionRawMutex, SdramStatus, 4> = Channel::new();
#[cfg(feature = "scanner-async")]
static DISPLAY_CHANNEL: Channel<CriticalSectionRawMutex, DisplayEvent, 4> = Channel::new();
#[cfg(feature = "scanner-async")]
static TOUCH_CHANNEL: Channel<CriticalSectionRawMutex, TouchEvent, 4> = Channel::new();
#[cfg(feature = "scanner-async")]
static COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, HostCommand, 8> = Channel::new();
#[cfg(feature = "scanner-async")]
static CDC_RESPONSE_CHANNEL: Channel<CriticalSectionRawMutex, CdcResponse, 8> = Channel::new();
#[cfg(feature = "scanner-async")]
static SHARED: Mutex<CriticalSectionRawMutex, SharedState> = Mutex::new(SharedState::new());
#[cfg(feature = "scanner-async")]
static DISPLAY_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[cfg(feature = "scanner-async")]
#[derive(Clone)]
pub struct ScanResult {
    pub data: Vec<u8>,
}

#[cfg(feature = "scanner-async")]
#[derive(Clone)]
pub struct SdramStatus {
    pub base_address: usize,
    pub test_passed: bool,
}

#[cfg(feature = "scanner-async")]
#[derive(Clone)]
pub enum DisplayEvent {
    Scan(ScanResult),
    Home,
    Error(String),
    Settings(ScannerSettings),
    Status(String),
    Qr(String),
}

#[cfg(feature = "scanner-async")]
#[derive(Clone)]
pub enum TouchEvent {
    Tap { x: u16, y: u16 },
}

#[cfg(feature = "scanner-async")]
#[derive(Clone)]
pub enum HostCommand {
    Trigger,
    Stop,
    GetSettings,
    SetSettings(ScannerSettings),
    DisplayQr(String),
    ShowSettings,
    ShowHome,
    EnterSettings,
    ScannerStatusCdc,
    ScannerDataCdc,
}

#[cfg(feature = "scanner-async")]
#[derive(Clone)]
pub enum CdcResponse {
    ScannerStatus { connected: bool, fw_byte: u8 },
    TriggerOk,
    TriggerFail,
    ScanData { data: Vec<u8>, type_byte: u8 },
    NoScanData,
    Settings { bits: u8 },
    SettingsReadFailed,
    SetSettingsResult { bits: u8 },
    SetSettingsWriteFailed,
    Ok,
    Error,
}

#[cfg(feature = "scanner-async")]
pub struct SharedState {
    pub scanner_connected: bool,
    pub model_str: [u8; 16],
    pub model_len: usize,
    pub last_scan: Option<Vec<u8>>,
    pub settings: Option<ScannerSettings>,
    pub auto_scan: bool,
}

#[cfg(feature = "scanner-async")]
impl SharedState {
    const fn new() -> Self {
        Self {
            scanner_connected: false,
            model_str: [0; 16],
            model_len: 0,
            last_scan: None,
            settings: None,
            auto_scan: false,
        }
    }
}

#[cfg(feature = "scanner-async")]
fn model_to_str(model: ScannerModel) -> &'static str {
    match model {
        ScannerModel::Gm65 => "GM65",
        ScannerModel::M3Y => "M3Y",
        ScannerModel::Generic => "Generic",
        ScannerModel::Unknown => "Unknown",
    }
}

#[cfg(feature = "scanner-async")]
fn write_hex(buf: &mut String, val: u64) {
    let hex = b"0123456789ABCDEF";
    let mut started = false;
    for i in (0..64).step_by(4).rev() {
        let digit = ((val >> i) & 0xF) as usize;
        if digit != 0 || started || i == 0 {
            started = true;
            let _ = buf.push(hex[digit] as char);
        }
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

    let mut config = Config::default();
    {
        config.rcc.hse = Some(Hse {
            freq: Hertz(8_000_000),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL168,
            divp: Some(PllPDiv::DIV2),
            divq: Some(PllQDiv::DIV7),
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.mux.clk48sel = mux::Clk48sel::PLL1_Q;
        config.rcc.pllsai = Some(Pll {
            prediv: PllPreDiv::DIV8,
            mul: PllMul::MUL384,
            divp: None,
            divq: None,
            divr: Some(PllRDiv::DIV7),
        });
    }
    let mut p = embassy_stm32::init(config);

    defmt::info!("Initializing SDRAM...");
    let sdram = SdramCtrl::new(&mut p, 168_000_000);
    let sdram_base = sdram.base_address();
    let sdram_ok = sdram.test_quick();
    defmt::info!("SDRAM: base={:#010x} test={}", sdram_base, sdram_ok);
    let _ = SDRAM_CHANNEL.try_send(SdramStatus {
        base_address: sdram_base,
        test_passed: sdram_ok,
    });

    defmt::info!("Initializing display...");
    let mut display = embassy_stm32f469i_disco::DisplayCtrl::new(&sdram, p.PH7, embassy_stm32f469i_disco::BoardHint::Auto);
    crate::display_async::render_status(&mut display.fb(), "Initializing...");

    let mut led = embassy_stm32::gpio::Output::new(
        p.PG6,
        embassy_stm32::gpio::Level::Low,
        embassy_stm32::gpio::Speed::Low,
    );

    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;
    let uart = usart::Uart::new_blocking(p.USART6, p.PG9, p.PG14, uart_config).unwrap();
    embassy_stm32::interrupt::USART6.disable();

    let async_uart = async_shared::AsyncUart {
        inner: uart,
        yield_threshold: 2_000_000,
    };
    let mut scanner = Gm65ScannerAsync::with_default_config(async_uart);

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
    usb_config_desc.product = Some("QR Scanner");
    usb_config_desc.serial_number = Some("f469disco");

    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut msos_descriptor = [0; 256];
    let mut control_buf = [0; 64];

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

    defmt::info!("Initializing touch controller...");
    let i2c_config = i2c::Config::default();
    let mut touch_i2c = i2c::I2c::new_blocking(p.I2C2, p.PB10, p.PB11, i2c_config);
    let touch_ctrl = TouchCtrl::new();
    let touch_ok = touch_ctrl.read_vendor_id(&mut touch_i2c).is_ok();
    defmt::info!("Touch: vendor_id read {}", touch_ok);

    let model_str;
    {
        let mut shared = SHARED.lock().await;
        match scanner.init().await {
            Ok(model) => {
                defmt::info!("Scanner: detected {:?}", model);
                shared.scanner_connected = true;
                model_str = model_to_str(model);
                let bytes = model_str.as_bytes();
                let model_len = bytes.len().min(16);
                shared.model_len = model_len;
                shared.model_str[..model_len].copy_from_slice(bytes);
            }
            Err(e) => {
                defmt::error!("Scanner: init failed {:?}", e);
                model_str = "Unknown";
                crate::display_async::render_error(&mut display.fb(), "Scanner init failed");
            }
        }
    }

    {
        let mut shared = SHARED.lock().await;
        if shared.scanner_connected {
            if let Some(settings) = scanner.get_scanner_settings().await {
                shared.settings = Some(settings);
                shared.auto_scan = true;
                crate::display_async::render_scanner_settings(&mut display.fb(), settings);
            } else {
                crate::display_async::render_home(&mut display.fb(), true, model_str);
            }
        } else {
            crate::display_async::render_home(&mut display.fb(), false, model_str);
        }
    }
    DISPLAY_READY.signal(());

    defmt::info!("Async scanner firmware started (168MHz, USB CDC, touch)");

    let scanner_task = async {
        loop {
            if let Ok(cmd) = COMMAND_CHANNEL.try_receive() {
                match cmd {
                    HostCommand::Trigger => {
                        defmt::info!("Scanner: host trigger");
                        if scanner.trigger_scan().await.is_err() {
                            defmt::error!("Scanner: trigger failed");
                            let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::TriggerFail);
                        } else {
                            let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::TriggerOk);
                        }
                    }
                    HostCommand::Stop => {
                        defmt::info!("Scanner: host stop");
                        scanner.cancel_scan();
                        let _ = scanner.stop_scan().await;
                    }
                    HostCommand::GetSettings => {
                        if let Some(s) = scanner.get_scanner_settings().await {
                            let mut shared = SHARED.lock().await;
                            shared.settings = Some(s);
                            let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Settings(s));
                            let _ = CDC_RESPONSE_CHANNEL
                                .try_send(CdcResponse::Settings { bits: s.bits() });
                        } else {
                            let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::SettingsReadFailed);
                            let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Error(String::from(
                                "Settings read failed",
                            )));
                        }
                    }
                    HostCommand::SetSettings(s) => {
                        scanner.set_scanner_settings(s).await;
                        Timer::after_millis(50).await;
                        if let Some(readback) = scanner.get_scanner_settings().await {
                            let mut shared = SHARED.lock().await;
                            shared.settings = Some(readback);
                            let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Settings(readback));
                            let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::SetSettingsResult {
                                bits: readback.bits(),
                            });
                        } else {
                            let _ =
                                CDC_RESPONSE_CHANNEL.try_send(CdcResponse::SetSettingsWriteFailed);
                        }
                    }
                    HostCommand::ShowSettings => {
                        if let Some(s) = scanner.get_scanner_settings().await {
                            let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Settings(s));
                        }
                    }
                    HostCommand::ShowHome => {
                        let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Home);
                    }
                    HostCommand::DisplayQr(text) => {
                        let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Qr(text));
                    }
                    HostCommand::EnterSettings => {
                        scanner.cancel_scan();
                        let _ = scanner.stop_scan().await;
                        if let Some(s) = scanner.get_scanner_settings().await {
                            let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Settings(s));
                        }
                        let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::Ok);
                    }
                    HostCommand::ScannerStatusCdc => {
                        scanner.cancel_scan();
                        let _ = scanner.stop_scan().await;
                        let shared = SHARED.lock().await;
                        let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::ScannerStatus {
                            connected: shared.scanner_connected,
                            fw_byte: 0x01,
                        });
                    }
                    HostCommand::ScannerDataCdc => {
                        let mut shared = SHARED.lock().await;
                        match shared.last_scan.take() {
                            Some(data) => {
                                let type_byte: u8 = match gm65_scanner::classify_payload(&data) {
                                    gm65_scanner::PayloadType::CashuV4 => 0x01,
                                    gm65_scanner::PayloadType::CashuV3 => 0x02,
                                    gm65_scanner::PayloadType::UrFragment => 0x03,
                                    gm65_scanner::PayloadType::PlainText
                                    | gm65_scanner::PayloadType::Url => 0x00,
                                    gm65_scanner::PayloadType::Binary => 0x04,
                                };
                                let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::ScanData {
                                    data: data.clone(),
                                    type_byte,
                                });
                            }
                            None => {
                                let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::NoScanData);
                            }
                        }
                    }
                }
                continue;
            }

            {
                let shared = SHARED.lock().await;
                if !shared.auto_scan {
                    Timer::after(Duration::from_millis(100)).await;
                    continue;
                }
            }

            if scanner.trigger_scan().await.is_err() {
                Timer::after(Duration::from_millis(500)).await;
                continue;
            }

            match embassy_time::with_timeout(Duration::from_secs(10), scanner.read_scan()).await {
                Ok(Some(data)) => {
                    let len = data.len();
                    defmt::info!("Scanner: scanned {} bytes", len);
                    let result = ScanResult { data: data.clone() };
                    let _ = SCAN_CHANNEL.try_send(result.clone());
                    let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Scan(result));
                    {
                        let mut shared = SHARED.lock().await;
                        shared.last_scan = Some(data);
                    }
                    for _ in 0..3 {
                        led.set_high();
                        Timer::after(Duration::from_millis(100)).await;
                        led.set_low();
                        Timer::after(Duration::from_millis(100)).await;
                    }
                }
                Ok(None) | Err(_) => {
                    scanner.cancel_scan();
                    let _ = scanner.stop_scan().await;
                }
            }
        }
    };

    let usb_task = async {
        usb_dev.run().await;
    };

    let cdc_task = async {
        use crate::cdc::{Command, FrameDecoder, Status};

        loop {
            cdc.wait_connection().await;
            defmt::info!("USB: connected");

            let mut heartbeat = Ticker::every(Duration::from_secs(3));
            let mut rx_buf = [0u8; 256];
            let mut frame_decoder = FrameDecoder::new();

            loop {
                if let Ok(result) = SCAN_CHANNEL.try_receive() {
                    let data_str = String::from_utf8_lossy(&result.data);
                    let payload = &result.data;
                    let type_byte: u8 = match gm65_scanner::classify_payload(payload) {
                        gm65_scanner::PayloadType::CashuV4 => 0x01,
                        gm65_scanner::PayloadType::CashuV3 => 0x02,
                        gm65_scanner::PayloadType::UrFragment => 0x03,
                        gm65_scanner::PayloadType::PlainText | gm65_scanner::PayloadType::Url => {
                            0x00
                        }
                        gm65_scanner::PayloadType::Binary => 0x04,
                    };
                    let mut msg = String::from("[SCAN] ");
                    msg.push_str(&data_str);
                    msg.push_str("\r\n");
                    match cdc.write_packet(msg.as_bytes()).await {
                        Ok(()) => {
                            let mut shared = SHARED.lock().await;
                            if shared.auto_scan {
                                shared.auto_scan = false;
                                Timer::after(Duration::from_millis(500)).await;
                                let mut shared = SHARED.lock().await;
                                shared.auto_scan = true;
                            }
                        }
                        Err(_) => break,
                    }
                    let _ = cdc.write_packet(&[type_byte]).await;
                    continue;
                }

                if let Ok(resp) = CDC_RESPONSE_CHANNEL.try_receive() {
                    match resp {
                        CdcResponse::ScannerStatus { connected, fw_byte } => {
                            let _ = cdc
                                .write_packet(&[
                                    Status::Ok.to_byte(),
                                    0,
                                    0,
                                    3,
                                    if connected { 1 } else { 0 },
                                    1,
                                    fw_byte,
                                ])
                                .await;
                        }
                        CdcResponse::TriggerOk => {
                            let _ = DISPLAY_CHANNEL
                                .try_send(DisplayEvent::Status(String::from("Scanning...")));
                            let _ = cdc.write_packet(&[Status::Ok.to_byte(), 0, 0]).await;
                        }
                        CdcResponse::TriggerFail => {
                            let _ = cdc
                                .write_packet(&[Status::ScannerNotConnected.to_byte(), 0, 0])
                                .await;
                        }
                        CdcResponse::ScanData { data, type_byte } => {
                            let len = data.len();
                            let mut buf = [0u8; 256];
                            buf[0] = type_byte;
                            let copy_len = len.min(255);
                            buf[1..copy_len + 1].copy_from_slice(&data[..copy_len]);
                            let _ = cdc
                                .write_packet(&[Status::Ok.to_byte(), 0, (copy_len + 1) as u8])
                                .await;
                            let _ = cdc.write_packet(&buf[..copy_len + 1]).await;
                            let _ = DISPLAY_CHANNEL
                                .try_send(DisplayEvent::Scan(ScanResult { data: data.clone() }));
                        }
                        CdcResponse::NoScanData => {
                            let _ = cdc
                                .write_packet(&[Status::NoScanData.to_byte(), 0, 0])
                                .await;
                        }
                        CdcResponse::Settings { bits } => {
                            let _ = cdc.write_packet(&[Status::Ok.to_byte(), 0, 1, bits]).await;
                        }
                        CdcResponse::SettingsReadFailed => {
                            let _ = cdc.write_packet(&[Status::Error.to_byte(), 0, 0]).await;
                        }
                        CdcResponse::SetSettingsResult { bits } => {
                            let _ = cdc.write_packet(&[Status::Ok.to_byte(), 0, 1, bits]).await;
                        }
                        CdcResponse::SetSettingsWriteFailed => {
                            let _ = cdc.write_packet(&[Status::Error.to_byte(), 0, 0]).await;
                        }
                        CdcResponse::Ok => {
                            let _ = cdc.write_packet(&[Status::Ok.to_byte(), 0, 0]).await;
                        }
                        CdcResponse::Error => {
                            let _ = cdc.write_packet(&[Status::Error.to_byte(), 0, 0]).await;
                        }
                    }
                    continue;
                }

                if let Ok(status) = SDRAM_CHANNEL.try_receive() {
                    let mut msg = String::from("[SDRAM] base=0x");
                    let _ = write_hex(&mut msg, status.base_address as u64);
                    msg.push_str(" test=");
                    if status.test_passed {
                        msg.push_str("PASS");
                    } else {
                        msg.push_str("FAIL");
                    }
                    msg.push_str("\r\n");
                    match cdc.write_packet(msg.as_bytes()).await {
                        Ok(()) => {}
                        Err(_) => break,
                    }
                    continue;
                }

                match cdc.read_packet(&mut rx_buf).await {
                    Ok(n) if n > 0 => {
                        if let Some(frame) = frame_decoder.decode(&rx_buf[..n]) {
                            match frame.command {
                                Command::ScannerStatus => {
                                    defmt::info!("CMD: SCANNER_STATUS");
                                    let _ = COMMAND_CHANNEL.try_send(HostCommand::ScannerStatusCdc);
                                }
                                Command::ScannerTrigger => {
                                    defmt::info!("CMD: SCANNER_TRIGGER");
                                    {
                                        let mut shared = SHARED.lock().await;
                                        shared.auto_scan = false;
                                    }
                                    let _ = COMMAND_CHANNEL.try_send(HostCommand::Trigger);
                                }
                                Command::ScannerData => {
                                    defmt::info!("CMD: SCANNER_DATA");
                                    let _ = COMMAND_CHANNEL.try_send(HostCommand::ScannerDataCdc);
                                }
                                Command::GetSettings => {
                                    defmt::info!("CMD: GET_SETTINGS");
                                    let _ = COMMAND_CHANNEL.try_send(HostCommand::GetSettings);
                                }
                                Command::SetSettings => {
                                    defmt::info!("CMD: SET_SETTINGS");
                                    let payload = frame.payload();
                                    if payload.is_empty() {
                                        let _ = cdc
                                            .write_packet(&[Status::InvalidPayload.to_byte(), 0, 0])
                                            .await;
                                    } else if let Some(settings) =
                                        ScannerSettings::from_bits(payload[0])
                                    {
                                        let _ = COMMAND_CHANNEL
                                            .try_send(HostCommand::SetSettings(settings));
                                    } else {
                                        let _ = cdc
                                            .write_packet(&[Status::InvalidPayload.to_byte(), 0, 0])
                                            .await;
                                    }
                                }
                                Command::DisplayQr => {
                                    defmt::info!("CMD: DISPLAY_QR");
                                    let text = core::str::from_utf8(frame.payload())
                                        .unwrap_or("<invalid utf8>");
                                    let _ = DISPLAY_CHANNEL
                                        .try_send(DisplayEvent::Qr(String::from(text)));
                                    let _ = cdc.write_packet(&[Status::Ok.to_byte(), 0, 0]).await;
                                }
                                Command::EnterSettings => {
                                    defmt::info!("CMD: ENTER_SETTINGS");
                                    let _ = COMMAND_CHANNEL.try_send(HostCommand::EnterSettings);
                                }
                            }
                            continue;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }

                heartbeat.next().await;
                match cdc.write_packet(b"[ALIVE] gm65-scanner ready\r\n").await {
                    Ok(()) => {}
                    Err(_) => break,
                }
            }
            defmt::info!("USB: disconnected");
            Timer::after(Duration::from_millis(100)).await;
        }
    };

    let display_task = async {
        DISPLAY_READY.wait().await;
        loop {
            let event = DISPLAY_CHANNEL.receive().await;
            match event {
                DisplayEvent::Scan(result) => {
                    let data_str = core::str::from_utf8(&result.data);
                    if data_str.is_ok() && result.data.len() <= 200 {
                        crate::qr_display_async::render_qr_mirror(&mut display.fb(), &result.data);
                    } else {
                        crate::display_async::render_scan_result(&mut display.fb(), &result.data);
                    }
                }
                DisplayEvent::Home => {
                    let shared = SHARED.lock().await;
                    let model = core::str::from_utf8(&shared.model_str[..shared.model_len])
                        .unwrap_or("Unknown");
                    crate::display_async::render_home(
                        &mut display.fb(),
                        shared.scanner_connected,
                        model,
                    );
                }
                DisplayEvent::Error(msg) => {
                    crate::display_async::render_error(&mut display.fb(), &msg);
                }
                DisplayEvent::Settings(s) => {
                    crate::display_async::render_scanner_settings(&mut display.fb(), s);
                }
                DisplayEvent::Status(msg) => {
                    crate::display_async::render_status(&mut display.fb(), &msg);
                }
                DisplayEvent::Qr(text) => {
                    if !crate::qr_display_async::render_qr_code(&mut display.fb(), &text) {
                        crate::display_async::render_error(&mut display.fb(), "QR encode failed");
                    }
                }
            }
        }
    };

    let touch_task = async {
        if !touch_ok {
            return;
        }
        loop {
            Timer::after(Duration::from_millis(50)).await;
            if let Ok(n) = touch_ctrl.td_status(&mut touch_i2c) {
                if n > 0 {
                    if let Ok(point) = touch_ctrl.get_touch(&mut touch_i2c) {
                        let _ = TOUCH_CHANNEL.try_send(TouchEvent::Tap {
                            x: point.x,
                            y: point.y,
                        });
                    }
                }
            }
        }
    };

    let settings_touch_task = async {
        if !touch_ok {
            return;
        }
        loop {
            match TOUCH_CHANNEL.try_receive() {
                Ok(TouchEvent::Tap { x, y }) => {
                    if y < 80 {
                        continue;
                    }

                    let back_y = 80u16 + 5 * 35;
                    if y >= back_y && y < back_y + 40 && x < 200 {
                        let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Home);
                        continue;
                    }

                    let row = ((y - 80) / 35) as usize;
                    let mut shared = SHARED.lock().await;
                    let mut settings = shared.settings.unwrap_or(ScannerSettings::default());

                    match row {
                        0 => {
                            settings ^= ScannerSettings::SOUND;
                        }
                        1 => {
                            settings ^= ScannerSettings::AIM;
                        }
                        2 => {
                            settings ^= ScannerSettings::LIGHT;
                        }
                        3 => {
                            settings ^= ScannerSettings::CONTINUOUS;
                        }
                        4 => {
                            settings ^= ScannerSettings::COMMAND;
                        }
                        _ => continue,
                    }

                    shared.settings = Some(settings);
                    let _ = COMMAND_CHANNEL.try_send(HostCommand::SetSettings(settings));
                }
                Err(_) => {
                    Timer::after(Duration::from_millis(100)).await;
                }
            }
        }
    };

    embassy_futures::join::join4(
        usb_task,
        embassy_futures::select::select(scanner_task, cdc_task),
        display_task,
        embassy_futures::select::select(touch_task, settings_touch_task),
    )
    .await;
}

#[cfg(not(feature = "scanner-async"))]
use defmt_rtt as _;

#[cfg(not(feature = "scanner-async"))]
#[cortex_m_rt::entry]
fn main() -> ! {
    defmt::error!("This binary requires the 'scanner-async' feature");
    loop {
        cortex_m::asm::wfi();
    }
}

#[path = "../display_utils.rs"]
mod display_utils;
mod display_async {
    const DISPLAY_CENTER_X: i32 = 240;
    const DISPLAY_MAX_Y: u32 = 800;
    include!("../display.rs");
}
mod qr_display_async {
    include!("../qr_display.rs");
}
mod cdc {
    #[cfg(feature = "scanner-async")]
    include!("../cdc.rs");
}
