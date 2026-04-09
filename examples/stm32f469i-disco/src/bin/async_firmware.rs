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

#[cfg(all(feature = "scanner-async", feature = "defmt"))]
use defmt_rtt as _;
#[cfg(all(feature = "scanner-async", feature = "defmt"))]
use panic_probe as _;
#[cfg(all(feature = "scanner-async", not(feature = "defmt")))]
use panic_halt as _;

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
use embassy_time::{Duration, Timer};
#[cfg(feature = "scanner-async")]
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
#[cfg(feature = "scanner-async")]
use embassy_usb::Builder;
#[cfg(feature = "scanner-async")]
use gm65_scanner::{Gm65ScannerAsync, ScannerModel, ScannerSettings};
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
const CHANNEL_CAPACITY: usize = 4;
#[cfg(feature = "scanner-async")]
const CMD_CHANNEL_CAPACITY: usize = 8;
#[cfg(feature = "scanner-async")]
const MODEL_STR_LEN: usize = 16;

#[cfg(feature = "scanner-async")]
const USB_BUF_SIZE: usize = 256;
#[cfg(feature = "scanner-async")]
const USB_SMALL_BUF_SIZE: usize = 64;
#[cfg(feature = "scanner-async")]
const MAX_PAYLOAD_COPY: usize = 255;
#[cfg(feature = "scanner-async")]
const QR_MAX_DATA_LEN: usize = 200;

#[cfg(feature = "scanner-async")]
const TOUCH_POLL_MS: u64 = 20;
#[cfg(feature = "scanner-async")]
const SETTINGS_COMMIT_DELAY_MS: u64 = 50;
#[cfg(feature = "scanner-async")]
const LED_BLINK_MS: u64 = 100;
#[cfg(feature = "scanner-async")]
const AUTO_SCAN_PAUSE_MS: u64 = 200;
#[cfg(feature = "scanner-async")]
const TRIGGER_RETRY_MS: u64 = 500;
#[cfg(feature = "scanner-async")]
const TOUCH_RETRY_DELAY_MS: u64 = 50;
#[cfg(feature = "scanner-async")]
const TOUCH_DEBOUNCE_MS: u64 = 200;
#[cfg(feature = "scanner-async")]
const USB_DISCONNECT_DELAY_MS: u64 = 100;

#[cfg(feature = "scanner-async")]
const HSE_FREQ_HZ: u32 = 8_000_000;
#[cfg(feature = "scanner-async")]
const SYSCLK_HZ: u32 = 180_000_000;
#[cfg(feature = "scanner-async")]
const UART_BAUD: u32 = 115200;

#[cfg(feature = "scanner-async")]
const USB_VID: u16 = 0xc0de;
#[cfg(feature = "scanner-async")]
const USB_PID: u16 = 0xcafe;

#[cfg(feature = "scanner-async")]
const DISPLAY_MAX_X: i32 = 479;
#[cfg(feature = "scanner-async")]
const SETTINGS_ROW_X_START: i32 = 10;
#[cfg(feature = "scanner-async")]
const SETTINGS_ROW_X_END: i32 = 460;

#[cfg(feature = "scanner-async")]
const LED_BLINK_COUNT: u8 = 3;

#[cfg(feature = "scanner-async")]
macro_rules! log_info {
    ($($arg:tt)*) => {
        #[cfg(feature = "defmt")]
        {
            defmt::info!($($arg)*);
        }
    };
}

#[cfg(feature = "scanner-async")]
macro_rules! log_error {
    ($($arg:tt)*) => {
        #[cfg(feature = "defmt")]
        {
            defmt::error!($($arg)*);
        }
    };
}

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
static SCAN_CHANNEL: Channel<CriticalSectionRawMutex, ScanResult, CHANNEL_CAPACITY> = Channel::new();
#[cfg(feature = "scanner-async")]
static SDRAM_CHANNEL: Channel<CriticalSectionRawMutex, SdramStatus, CHANNEL_CAPACITY> = Channel::new();
#[cfg(feature = "scanner-async")]
static DISPLAY_CHANNEL: Channel<CriticalSectionRawMutex, DisplayEvent, CHANNEL_CAPACITY> = Channel::new();
#[cfg(feature = "scanner-async")]
static TOUCH_CHANNEL: Channel<CriticalSectionRawMutex, TouchEvent, CHANNEL_CAPACITY> = Channel::new();
#[cfg(feature = "scanner-async")]
static COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, HostCommand, CMD_CHANNEL_CAPACITY> = Channel::new();
#[cfg(feature = "scanner-async")]
static CDC_RESPONSE_CHANNEL: Channel<CriticalSectionRawMutex, CdcResponse, CMD_CHANNEL_CAPACITY> = Channel::new();
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
    ScannerStatus([u8; 3]),
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
    pub scanner_initialized: bool,
    pub model: ScannerModel,
    pub model_str: [u8; MODEL_STR_LEN],
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
            scanner_initialized: false,
            model: ScannerModel::Unknown,
            model_str: [0; MODEL_STR_LEN],
            model_len: 0,
            last_scan: None,
            settings: None,
            auto_scan: false,
        }
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
            freq: Hertz(HSE_FREQ_HZ),
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
            divp: None,
            divq: Some(PllQDiv::DIV8),
            divr: Some(PllRDiv::DIV7),
        });
    }
    let mut p = embassy_stm32::init(config);

    // WORKAROUND: embassy-stm32 writes CK48MSEL to DCKCFGR (wrong register on STM32F469).
    // USB 48MHz mux is in DCKCFGR2 bit 27, not DCKCFGR bit 27.
    // Without this, USB uses default PLL1_Q (51.4MHz at 180MHz), which fails enumeration.
    // See: https://github.com/Amperstrand/gm65-scanner/issues/23
    stm32_metapac::RCC.dckcfgr2().modify(|w| {
        w.set_clk48sel(mux::Clk48sel::PLLSAI1_Q);
    });

    log_info!("Initializing SDRAM...");
    let sdram = SdramCtrl::new(&mut p, SYSCLK_HZ);
    let sdram_base = sdram.base_address();
    let sdram_ok = sdram.test_quick();
    log_info!("SDRAM: base={:#010x} test={}", sdram_base, sdram_ok);
    let _ = SDRAM_CHANNEL.try_send(SdramStatus {
        base_address: sdram_base,
        test_passed: sdram_ok,
    });

    log_info!("Initializing display...");
    let mut display = embassy_stm32f469i_disco::DisplayCtrl::new(&sdram, p.LTDC, p.DSIHOST, p.PJ2, p.PH7, embassy_stm32f469i_disco::BoardHint::ForceNt35510);
    crate::display_async::render_status(&mut display.fb(), "Initializing...");

    let mut led = embassy_stm32::gpio::Output::new(
        p.PG6,
        embassy_stm32::gpio::Level::Low,
        embassy_stm32::gpio::Speed::Low,
    );

    embassy_stm32::interrupt::USART6.disable();
    let mut uart_config = usart::Config::default();
    uart_config.baudrate = UART_BAUD;
    let uart = usart::Uart::new_blocking(p.USART6, p.PG9, p.PG14, uart_config).unwrap();

    let async_uart = async_shared::AsyncUart {
        inner: uart,
    };

    let mut ep_out_buffer = [0u8; USB_BUF_SIZE];
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

    let mut usb_config_desc = embassy_usb::Config::new(USB_VID, USB_PID);
    usb_config_desc.manufacturer = Some("gm65-scanner");
    usb_config_desc.product = Some("QR Scanner");
    usb_config_desc.serial_number = Some("f469disco");

    let mut config_descriptor = [0; USB_BUF_SIZE];
    let mut bos_descriptor = [0; USB_BUF_SIZE];
    let mut msos_descriptor = [0; USB_BUF_SIZE];
    let mut control_buf = [0; USB_SMALL_BUF_SIZE];

    let mut usb_state = State::new();
    let mut usb_builder = Builder::new(
        usb_driver,
        usb_config_desc,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );
    let mut cdc = CdcAcmClass::new(&mut usb_builder, &mut usb_state, USB_SMALL_BUF_SIZE as u16);
    let mut usb_dev = usb_builder.build();

    log_info!("Initializing touch controller...");
    let i2c_config = i2c::Config::default();
    let mut touch_i2c = i2c::I2c::new_blocking(p.I2C1, p.PB8, p.PB9, i2c_config);
    let touch_ctrl = TouchCtrl::new();
    let touch_ok = touch_ctrl.read_vendor_id(&mut touch_i2c).is_ok();
    log_info!("Touch: vendor_id read {}", touch_ok);

    {
        let mut shared = SHARED.lock().await;
        shared.auto_scan = true;
    }

    DISPLAY_READY.signal(());

    log_info!("Async scanner firmware started (168MHz, USB CDC, touch)");

    // Phase 1: Start USB polling and scanner init concurrently.
    // USB must be polling before the host enumerates, otherwise
    // SET_CONFIGURATION is missed and wait_connection() hangs forever.
    // Scanner init runs alongside USB with only 2 tasks competing,
    // giving the UART reads enough executor attention to succeed.
    use embassy_futures::select::{select, Either};
    let mut scanner = Gm65ScannerAsync::with_default_config(async_uart);
    use gm65_scanner::ScannerDriver;

    let init_result = select(usb_dev.run(), scanner.init()).await;
    match init_result {
        Either::First(_) => unreachable!(),
        Either::Second(result) => {
            match result {
                Ok(model) => {
                    log_info!("Scanner: detected {:?}", model);
                    let model_str = scanner_utils::model_to_str(model);
                    {
                        let mut shared = SHARED.lock().await;
                        shared.scanner_connected = true;
                        shared.scanner_initialized = true;
                        shared.model = model;
                        let bytes = model_str.as_bytes();
                        let model_len = bytes.len().min(MODEL_STR_LEN);
                        shared.model_len = model_len;
                        shared.model_str[..model_len].copy_from_slice(bytes);
                    }
                    if let Some(settings) = scanner.get_scanner_settings().await {
                        let mut shared = SHARED.lock().await;
                        shared.settings = Some(settings);
                        let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Settings(settings));
                    } else {
                        let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Home);
                    }
                }
                Err(_e) => {
                    log_error!("Scanner: init failed {:?}", _e);
                    let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Error(alloc::string::String::from("Scanner init failed")));
                }
            }
        }
    }

    let scanner_task = async {
        let mut pending_cmd: Option<HostCommand> = None;

        loop {
            let cmd = if let Some(cmd) = pending_cmd.take() {
                Some(cmd)
            } else {
                COMMAND_CHANNEL.try_receive().ok()
            };

            if let Some(cmd) = cmd {
                match cmd {
                    HostCommand::Trigger => {
                        log_info!("Scanner: host trigger");
                        if scanner.trigger_scan().await.is_err() {
                            log_error!("Scanner: trigger failed");
                            let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::TriggerFail);
                        } else {
                            let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::TriggerOk);
                        }
                    }
                    HostCommand::Stop => {
                        log_info!("Scanner: host stop");
                        scanner.cancel_scan();
                        let _ = scanner.stop_scan().await;
                    }
                    HostCommand::GetSettings => {
                        scanner.cancel_scan();
                        let _ = scanner.stop_scan().await;
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
                        scanner.cancel_scan();
                        let _ = scanner.stop_scan().await;
                        scanner.set_scanner_settings(s).await;
                        Timer::after_millis(SETTINGS_COMMIT_DELAY_MS).await;
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
                        scanner.cancel_scan();
                        let _ = scanner.stop_scan().await;
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
                        let model_byte = scanner_utils::model_to_status_byte(shared.model);
                        let payload = scanner_utils::build_scanner_status_payload(
                            shared.scanner_connected,
                            shared.scanner_initialized,
                            model_byte,
                        );
                        let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::ScannerStatus(payload));
                    }
                    HostCommand::ScannerDataCdc => {
                        let mut shared = SHARED.lock().await;
                        match shared.last_scan.take() {
                            Some(data) => {
                                let type_byte = scanner_utils::payload_type_to_byte(
                                    gm65_scanner::classify_payload(&data),
                                );
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

            let auto_scan = {
                let shared = SHARED.lock().await;
                shared.auto_scan
            };

            if !auto_scan {
                let cmd = COMMAND_CHANNEL.receive().await;
                pending_cmd = Some(cmd);
                continue;
            }

            if scanner.trigger_scan().await.is_err() {
                Timer::after(Duration::from_millis(TRIGGER_RETRY_MS)).await;
                continue;
            }

            match embassy_futures::select::select(scanner.read_scan(), COMMAND_CHANNEL.receive()).await {
                embassy_futures::select::Either::First(result) => {
                    match result {
                        Some(data) => {
                            let _len = data.len();
                            log_info!("Scanner: scanned {} bytes", _len);
                            let result = ScanResult { data: data.clone() };
                            let _ = SCAN_CHANNEL.try_send(result.clone());
                            let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Scan(result));
                            {
                                let mut shared = SHARED.lock().await;
                                shared.last_scan = Some(data);
                            }
                            for _ in 0..LED_BLINK_COUNT {
                                led.set_high();
                                Timer::after(Duration::from_millis(LED_BLINK_MS)).await;
                                led.set_low();
                                Timer::after(Duration::from_millis(LED_BLINK_MS)).await;
                            }
                        }
                        None => {
                            scanner.cancel_scan();
                            let _ = scanner.stop_scan().await;
                        }
                    }
                }
                embassy_futures::select::Either::Second(cmd) => {
                    scanner.cancel_scan();
                    pending_cmd = Some(cmd);
                }
            }
        }
    };

    let cdc_task = async {
        use crate::cdc::{Command, FrameDecoder, Status};

        macro_rules! write_cdc_response {
            ($resp:expr) => {
                match $resp {
                    CdcResponse::ScannerStatus(payload) => {
                        let _ = cdc
                            .write_packet(&[Status::Ok.to_byte(), 0, payload.len() as u8])
                            .await;
                        let _ = cdc.write_packet(&payload).await;
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
                        let mut buf = [0u8; USB_BUF_SIZE];
                        buf[0] = type_byte;
                        let copy_len = len.min(MAX_PAYLOAD_COPY);
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
            };
        }

        loop {
            cdc.wait_connection().await;
            log_info!("USB: connected");

            let mut rx_buf = [0u8; USB_BUF_SIZE];
            let mut frame_decoder = FrameDecoder::new();

            loop {
                if let Ok(result) = SCAN_CHANNEL.try_receive() {
                    let data_str = String::from_utf8_lossy(&result.data);
                    let payload = &result.data;
                    let type_byte = scanner_utils::payload_type_to_byte(
                        gm65_scanner::classify_payload(payload),
                    );
                    let mut msg = String::from("[SCAN] ");
                    msg.push_str(&data_str);
                    msg.push_str("\r\n");
                    match cdc.write_packet(msg.as_bytes()).await {
                        Ok(()) => {
                            {
                                let mut shared = SHARED.lock().await;
                                shared.auto_scan = false;
                            }
                            Timer::after(Duration::from_millis(AUTO_SCAN_PAUSE_MS)).await;
                            {
                                let mut shared = SHARED.lock().await;
                                shared.auto_scan = true;
                            }
                        }
                        Err(_) => break,
                    }
                    let _ = cdc.write_packet(&[type_byte]).await;
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
                                    log_info!("CMD: SCANNER_STATUS");
                                    let _ = COMMAND_CHANNEL.try_send(HostCommand::ScannerStatusCdc);
                                    let resp = CDC_RESPONSE_CHANNEL.receive().await;
                                    write_cdc_response!(resp);
                                }
                                Command::ScannerTrigger => {
                                    log_info!("CMD: SCANNER_TRIGGER");
                                    {
                                        let mut shared = SHARED.lock().await;
                                        shared.auto_scan = false;
                                    }
                                    let _ = COMMAND_CHANNEL.try_send(HostCommand::Trigger);
                                    let resp = CDC_RESPONSE_CHANNEL.receive().await;
                                    write_cdc_response!(resp);
                                }
                                Command::ScannerData => {
                                    log_info!("CMD: SCANNER_DATA");
                                    let _ = COMMAND_CHANNEL.try_send(HostCommand::ScannerDataCdc);
                                    let resp = CDC_RESPONSE_CHANNEL.receive().await;
                                    write_cdc_response!(resp);
                                }
                                Command::GetSettings => {
                                    log_info!("CMD: GET_SETTINGS");
                                    let _ = COMMAND_CHANNEL.try_send(HostCommand::GetSettings);
                                    let resp = CDC_RESPONSE_CHANNEL.receive().await;
                                    write_cdc_response!(resp);
                                }
                                Command::SetSettings => {
                                    log_info!("CMD: SET_SETTINGS");
                                    let payload = frame.payload();
                                    if payload.is_empty() {
                                        let _ = cdc
                                            .write_packet(&[Status::InvalidPayload.to_byte(), 0, 0])
                                            .await;
                                    } else if let Some(settings) =
                                        ScannerSettings::from_bits(payload[0])
                                    {
                                        let _ = COMMAND_CHANNEL.try_send(HostCommand::SetSettings(settings));
                                        let resp = CDC_RESPONSE_CHANNEL.receive().await;
                                        write_cdc_response!(resp);
                                    } else {
                                        let _ = cdc
                                            .write_packet(&[Status::InvalidPayload.to_byte(), 0, 0])
                                            .await;
                                    }
                                }
                                Command::DisplayQr => {
                                    log_info!("CMD: DISPLAY_QR");
                                    let text = core::str::from_utf8(frame.payload())
                                        .unwrap_or("<invalid utf8>");
                                    let _ = DISPLAY_CHANNEL
                                        .try_send(DisplayEvent::Qr(String::from(text)));
                                    let _ = cdc.write_packet(&[Status::Ok.to_byte(), 0, 0]).await;
                                }
                                Command::EnterSettings => {
                                    log_info!("CMD: ENTER_SETTINGS");
                                    let _ = COMMAND_CHANNEL.try_send(HostCommand::EnterSettings);
                                    let resp = CDC_RESPONSE_CHANNEL.receive().await;
                                    write_cdc_response!(resp);
                                }
                            }
                            continue;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }

            }
            log_info!("USB: disconnected");
            Timer::after(Duration::from_millis(USB_DISCONNECT_DELAY_MS)).await;
        }
    };

    let display_task = async {
        DISPLAY_READY.wait().await;
        loop {
            let event = DISPLAY_CHANNEL.receive().await;
            match event {
                DisplayEvent::Scan(result) => {
                    let data_str = core::str::from_utf8(&result.data);
                    if data_str.is_ok() && result.data.len() <= QR_MAX_DATA_LEN {
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
        const TOUCH_MARGIN: u16 = 3;
        let mut finger_down = false;
        let mut pending_tap: Option<(u16, u16)> = None;

        loop {
            Timer::after(embassy_time::Duration::from_millis(TOUCH_POLL_MS)).await;
            if let Ok(n) = touch_ctrl.td_status(&mut touch_i2c) {
                if n > 0 {
                    if let Ok(point) = touch_ctrl.get_touch(&mut touch_i2c) {
                        let tx = point.x;
                        let ty = point.y;
                        // FT6X06 reports phantom touches at edges (BSP touch.rs)
                        if tx >= TOUCH_MARGIN
                            && i32::from(tx) <= DISPLAY_MAX_X - i32::from(TOUCH_MARGIN)
                            && ty >= TOUCH_MARGIN
                            && ty <= 799 - TOUCH_MARGIN
                        {
                            pending_tap = Some((tx, ty));
                        }
                        finger_down = true;
                    }
                } else if finger_down {
                    if let Some((x, y)) = pending_tap.take() {
                        let _ = TOUCH_CHANNEL.try_send(TouchEvent::Tap { x, y });
                    }
                    finger_down = false;
                    Timer::after(Duration::from_millis(TOUCH_DEBOUNCE_MS)).await;
                }
            }
        }
    };

    let settings_touch_task = async {
        if !touch_ok {
            return;
        }
        // Hit zones must match display.rs render_scanner_settings() layout
        const ROW_SPACING: u16 = 90;
        const ROW_Y_START: u16 = 120;
        const ROW_Y_END: u16 = 570;
        const BACK_Y: u16 = 715;
        const BACK_Y_END: u16 = 765;
        const BACK_X_START: u16 = 40;
        const BACK_X_END: u16 = 240;

        loop {
            match TOUCH_CHANNEL.try_receive() {
                Ok(TouchEvent::Tap { x, y }) => {
                    if y >= BACK_Y
                        && y < BACK_Y_END
                        && x >= BACK_X_START
                        && x < BACK_X_END
                    {
                        let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Home);
                        continue;
                    }

                    if y >= ROW_Y_START
                        && y < ROW_Y_END
                        && i32::from(x) >= SETTINGS_ROW_X_START
                        && i32::from(x) < SETTINGS_ROW_X_END
                    {
                        let row = ((y - ROW_Y_START) / ROW_SPACING) as usize;
                        let mut shared = SHARED.lock().await;
                        let mut settings =
                            shared.settings.unwrap_or(ScannerSettings::default());

                        let Some(flag) = scanner_utils::row_to_settings_flag(row) else {
                            continue;
                        };
                        settings ^= flag;

                        shared.settings = Some(settings);
                        let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Settings(settings));
                        let _ =
                            COMMAND_CHANNEL.try_send(HostCommand::SetSettings(settings));
                    }
                }
                Err(_) => {
                    Timer::after(Duration::from_millis(TOUCH_RETRY_DELAY_MS)).await;
                }
            }
        }
    };

    embassy_futures::join::join4(
        usb_dev.run(),
        embassy_futures::select::select(scanner_task, cdc_task),
        display_task,
        embassy_futures::select::select(touch_task, settings_touch_task),
    )
    .await;
}

#[cfg(all(not(feature = "scanner-async"), feature = "defmt"))]
use defmt_rtt as _;
#[cfg(all(not(feature = "scanner-async"), not(feature = "defmt")))]
use panic_halt as _;

#[cfg(not(feature = "scanner-async"))]
#[cortex_m_rt::entry]
fn main() -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}

#[path = "../display_utils.rs"]
mod display_utils;
#[path = "../scanner_utils.rs"]
mod scanner_utils;
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
