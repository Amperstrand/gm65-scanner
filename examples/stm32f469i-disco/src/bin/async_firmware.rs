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
use core::sync::atomic::{AtomicU8, Ordering};

#[cfg(all(feature = "scanner-async", feature = "defmt"))]
use defmt_rtt as _;
#[cfg(all(feature = "scanner-async", not(feature = "defmt")))]
use panic_halt as _;
#[cfg(all(feature = "scanner-async", feature = "defmt"))]
use panic_probe as _;

#[cfg(feature = "scanner-async")]
use embassy_executor::Spawner;
#[cfg(feature = "scanner-async")]
use embassy_stm32::flash::Flash;
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
use embassy_usb::class::cdc_acm::{CdcAcmClass, State as CdcState};
#[cfg(feature = "scanner-async")]
use embassy_usb::class::hid::{
    Config as HidConfig, HidBootProtocol, HidProtocolMode, HidReaderWriter, HidSubclass, HidWriter,
    ReportId, RequestHandler, State as HidState,
};
#[cfg(feature = "scanner-async")]
use embassy_usb::control::OutResponse;
#[cfg(feature = "scanner-async")]
use embassy_usb::Builder;
#[cfg(feature = "scanner-async")]
use gm65_scanner::hid::pos::POS_BARCODE_SCANNER_REPORT_DESCRIPTOR;
#[cfg(feature = "scanner-async")]
use gm65_scanner::{Gm65ScannerAsync, ScannerModel, ScannerSettings};
#[cfg(feature = "scanner-async")]
use linked_list_allocator::LockedHeap;

#[cfg(feature = "scanner-async")]
use embassy_stm32f469i_disco::display::SdramCtrl;
#[cfg(feature = "scanner-async")]
use embassy_stm32f469i_disco::TouchCtrl;

#[path = "../flash_store.rs"]
#[cfg(feature = "scanner-async")]
mod flash_store;

#[cfg(feature = "scanner-async")]
use stm32f469i_disco_scanner::compatibility;
#[cfg(feature = "scanner-async")]
use stm32f469i_disco_scanner::feedback::{profile_save_status, pulses_for, FeedbackEvent};
#[cfg(feature = "scanner-async")]
use stm32f469i_disco_scanner::hid_pos_profile::build_pos_report;
#[cfg(feature = "scanner-async")]
use stm32f469i_disco_scanner::keyboard_profile::build_keyboard_reports;
#[cfg(feature = "scanner-async")]
use stm32f469i_disco_scanner::touch_ui::{apply_action as apply_touch_action, map_touch_to_action};
#[cfg(feature = "scanner-async")]
use stm32f469i_disco_scanner::usb_personality::info_for_profile;

mod async_shared {
    #[cfg(feature = "scanner-async")]
    include!("../async_shared.rs");
}

#[cfg(feature = "scanner-async")]
use embassy_stm32::bind_interrupts;

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
    FLASH => embassy_stm32::flash::InterruptHandler;
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
static PROFILE_CHANNEL: Channel<CriticalSectionRawMutex, ProfileCommand, 4> = Channel::new();
#[cfg(feature = "scanner-async")]
static FEEDBACK_CHANNEL: Channel<CriticalSectionRawMutex, FeedbackEvent, 8> = Channel::new();
#[cfg(feature = "scanner-async")]
static SHARED: Mutex<CriticalSectionRawMutex, SharedState> = Mutex::new(SharedState::new());
#[cfg(feature = "scanner-async")]
static DISPLAY_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();
#[cfg(feature = "scanner-async")]
static KEYBOARD_LED_STATE: AtomicU8 = AtomicU8::new(0);
#[cfg(feature = "scanner-async")]
static KEYBOARD_PROTOCOL_MODE: AtomicU8 = AtomicU8::new(HidProtocolMode::Report as u8);

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
    Compatibility(compatibility::CompatibilityProfile),
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
    GetCompatibilityProfile,
    SetCompatibilityProfile(compatibility::UsbMode),
    RebootUsb,
    GetHostOptions,
    SetHostOptions(compatibility::CompatibilityProfile),
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
    CompatibilityProfile { mode: compatibility::UsbMode },
    HostOptions(compatibility::CompatibilityProfile),
    RebootRequired,
}

#[cfg(feature = "scanner-async")]
#[derive(Clone, Copy)]
pub enum ProfileCommand {
    Save {
        profile: compatibility::CompatibilityProfile,
        reboot: bool,
    },
}

#[cfg(feature = "scanner-async")]
pub struct SharedState {
    pub scanner_connected: bool,
    pub model_str: [u8; 16],
    pub model_len: usize,
    pub last_scan: Option<Vec<u8>>,
    pub settings: Option<ScannerSettings>,
    pub auto_scan: bool,
    pub profile: compatibility::CompatibilityProfile,
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
            profile: compatibility::CompatibilityProfile {
                usb_mode: compatibility::UsbMode::Ds2208KeyboardHid,
                suffix: compatibility::SuffixMode::Enter,
                key_delay_ms: 0,
                case_mode: compatibility::CaseMode::Preserve,
                fast_hid: true,
                caps_lock_override: true,
                simulated_caps_lock: false,
                scanner_settings: 0xC1,
                prefix_len: 0,
                suffix_bytes_len: 0,
                prefix: [0; compatibility::PROFILE_PREFIX_MAX],
                suffix_bytes: [0; compatibility::PROFILE_SUFFIX_MAX],
            },
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
struct KeyboardRequestHandler;

#[cfg(feature = "scanner-async")]
impl RequestHandler for KeyboardRequestHandler {
    fn get_report(&mut self, _id: ReportId, _buf: &mut [u8]) -> Option<usize> {
        None
    }

    fn set_report(&mut self, _id: ReportId, data: &[u8]) -> OutResponse {
        if let Some(&leds) = data.first() {
            KEYBOARD_LED_STATE.store(leds, Ordering::Relaxed);
        }
        OutResponse::Accepted
    }

    fn get_protocol(&self) -> HidProtocolMode {
        HidProtocolMode::from(KEYBOARD_PROTOCOL_MODE.load(Ordering::Relaxed))
    }

    fn set_protocol(&mut self, protocol: HidProtocolMode) -> OutResponse {
        KEYBOARD_PROTOCOL_MODE.store(protocol as u8, Ordering::Relaxed);
        OutResponse::Accepted
    }
}

#[cfg(feature = "scanner-async")]
async fn reboot_device_after_delay() -> ! {
    Timer::after(Duration::from_millis(100)).await;
    cortex_m::peripheral::SCB::sys_reset();
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

    log_info!("Initializing SDRAM...");
    let sdram = SdramCtrl::new(&mut p, 168_000_000);
    let sdram_base = sdram.base_address();
    let sdram_ok = sdram.test_quick();
    log_info!("SDRAM: base={:#010x} test={}", sdram_base, sdram_ok);
    let _ = SDRAM_CHANNEL.try_send(SdramStatus {
        base_address: sdram_base,
        test_passed: sdram_ok,
    });

    log_info!("Initializing display...");
    let mut display = embassy_stm32f469i_disco::DisplayCtrl::new(
        &sdram,
        p.PH7,
        embassy_stm32f469i_disco::BoardHint::Auto,
    );
    crate::display_async::render_status(&mut display.fb(), "Initializing...");

    let mut led = embassy_stm32::gpio::Output::new(
        p.PG6,
        embassy_stm32::gpio::Level::Low,
        embassy_stm32::gpio::Speed::Low,
    );

    embassy_stm32::interrupt::USART6.disable();
    let mut uart_config = usart::Config::default();
    uart_config.baudrate = 115200;
    let uart = usart::Uart::new_blocking(p.USART6, p.PG9, p.PG14, uart_config).unwrap();
    embassy_stm32::interrupt::USART6.disable();

    let async_uart = async_shared::AsyncUart {
        inner: uart,
        yield_threshold: 500_000,
    };

    let flash = Flash::new(p.FLASH, Irqs);
    let mut flash_store = flash_store::FlashStore::new(flash);
    let active_profile = flash_store.load_blocking();

    log_info!("Initializing touch controller...");
    let i2c_config = i2c::Config::default();
    let mut touch_i2c = i2c::I2c::new_blocking(p.I2C2, p.PB10, p.PB11, i2c_config);
    let touch_ctrl = TouchCtrl::new();
    let touch_ok = touch_ctrl.read_vendor_id(&mut touch_i2c).is_ok();
    log_info!("Touch: vendor_id read {}", touch_ok);

    {
        let mut shared = SHARED.lock().await;
        shared.auto_scan = true;
        shared.profile = active_profile;
    }

    DISPLAY_READY.signal(());
    let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Compatibility(active_profile));
    let _ = FEEDBACK_CHANNEL.try_send(FeedbackEvent::PowerUp);

    log_info!("Async scanner firmware started (168MHz, touch, DS2208 profile)");

    let scanner_task = async {
        let mut scanner = Gm65ScannerAsync::with_default_config(async_uart);
        use gm65_scanner::ScannerDriver;

        match scanner.init().await {
            Ok(model) => {
                log_info!("Scanner: detected {:?}", model);
                let model_str = model_to_str(model);
                let mut shared = SHARED.lock().await;
                shared.scanner_connected = true;
                if let Some(profile_settings) =
                    ScannerSettings::from_bits(shared.profile.scanner_settings)
                {
                    scanner.set_scanner_settings(profile_settings).await;
                    shared.settings = Some(profile_settings);
                }
                let bytes = model_str.as_bytes();
                let model_len = bytes.len().min(16);
                shared.model_len = model_len;
                shared.model_str[..model_len].copy_from_slice(bytes);
                let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Compatibility(shared.profile));
            }
            Err(_e) => {
                log_error!("Scanner: init failed {:?}", _e);
                let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Error(alloc::string::String::from(
                    "Scanner init failed",
                )));
            }
        }

        loop {
            if let Ok(cmd) = COMMAND_CHANNEL.try_receive() {
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
                            shared.profile.scanner_settings = readback.bits();
                            let _ = DISPLAY_CHANNEL
                                .try_send(DisplayEvent::Compatibility(shared.profile));
                            let _ = PROFILE_CHANNEL.try_send(ProfileCommand::Save {
                                profile: shared.profile,
                                reboot: false,
                            });
                            let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::SetSettingsResult {
                                bits: readback.bits(),
                            });
                        } else {
                            let _ =
                                CDC_RESPONSE_CHANNEL.try_send(CdcResponse::SetSettingsWriteFailed);
                        }
                    }
                    HostCommand::ShowSettings => {
                        let shared = SHARED.lock().await;
                        let _ =
                            DISPLAY_CHANNEL.try_send(DisplayEvent::Compatibility(shared.profile));
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
                        let shared = SHARED.lock().await;
                        let _ =
                            DISPLAY_CHANNEL.try_send(DisplayEvent::Compatibility(shared.profile));
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
                    HostCommand::GetCompatibilityProfile => {
                        let shared = SHARED.lock().await;
                        let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::CompatibilityProfile {
                            mode: shared.profile.usb_mode,
                        });
                    }
                    HostCommand::SetCompatibilityProfile(mode) => {
                        let mut shared = SHARED.lock().await;
                        shared.profile.usb_mode = mode;
                        let _ =
                            DISPLAY_CHANNEL.try_send(DisplayEvent::Compatibility(shared.profile));
                        let _ = PROFILE_CHANNEL.try_send(ProfileCommand::Save {
                            profile: shared.profile,
                            reboot: true,
                        });
                        let _ = FEEDBACK_CHANNEL.try_send(FeedbackEvent::ConfigOk);
                        let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::RebootRequired);
                    }
                    HostCommand::GetHostOptions => {
                        let shared = SHARED.lock().await;
                        let _ =
                            CDC_RESPONSE_CHANNEL.try_send(CdcResponse::HostOptions(shared.profile));
                    }
                    HostCommand::SetHostOptions(profile) => {
                        let mut shared = SHARED.lock().await;
                        let reboot = shared.profile.needs_reenumeration_to(profile);
                        shared.profile = profile;
                        if let Some(settings) =
                            ScannerSettings::from_bits(shared.profile.scanner_settings)
                        {
                            scanner.set_scanner_settings(settings).await;
                            shared.settings = Some(settings);
                        }
                        let _ =
                            DISPLAY_CHANNEL.try_send(DisplayEvent::Compatibility(shared.profile));
                        let _ = PROFILE_CHANNEL.try_send(ProfileCommand::Save {
                            profile: shared.profile,
                            reboot,
                        });
                        let _ = FEEDBACK_CHANNEL.try_send(FeedbackEvent::ConfigOk);
                        let _ = CDC_RESPONSE_CHANNEL.try_send(if reboot {
                            CdcResponse::RebootRequired
                        } else {
                            CdcResponse::HostOptions(shared.profile)
                        });
                    }
                    HostCommand::RebootUsb => {
                        let shared = SHARED.lock().await;
                        let _ = PROFILE_CHANNEL.try_send(ProfileCommand::Save {
                            profile: shared.profile,
                            reboot: true,
                        });
                        let _ = CDC_RESPONSE_CHANNEL.try_send(CdcResponse::RebootRequired);
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
                    let _len = data.len();
                    log_info!("Scanner: scanned {} bytes", _len);
                    let result = ScanResult { data: data.clone() };
                    let _ = SCAN_CHANNEL.try_send(result.clone());
                    let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Scan(result));
                    {
                        let mut shared = SHARED.lock().await;
                        shared.last_scan = Some(data);
                    }
                    let _ = FEEDBACK_CHANNEL.try_send(FeedbackEvent::DecodeOk);
                }
                Ok(None) | Err(_) => {
                    scanner.cancel_scan();
                    let _ = scanner.stop_scan().await;
                }
            }
        }
    };

    let personality_task = async {
        match active_profile.usb_mode {
            compatibility::UsbMode::AdminCdc => {
                use crate::cdc::{Command, FrameDecoder, Status};
                let personality = info_for_profile(active_profile);

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
                usb_config_desc.product = Some(personality.product);
                usb_config_desc.serial_number = Some(personality.serial);
                let mut config_descriptor = [0; 256];
                let mut bos_descriptor = [0; 256];
                let mut msos_descriptor = [0; 256];
                let mut control_buf = [0; 64];
                let mut usb_state = CdcState::new();
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

                let usb_task = async { usb_dev.run().await };
                let cdc_task = async {
                    loop {
                        cdc.wait_connection().await;
                        let mut heartbeat = Ticker::every(Duration::from_secs(3));
                        let mut rx_buf = [0u8; 256];
                        let mut frame_decoder = FrameDecoder::new();
                        loop {
                            if let Ok(result) = SCAN_CHANNEL.try_receive() {
                                let data_str = String::from_utf8_lossy(&result.data);
                                let mut msg = String::from("[SCAN] ");
                                msg.push_str(&data_str);
                                msg.push_str("\r\n");
                                if cdc.write_packet(msg.as_bytes()).await.is_err() {
                                    break;
                                }
                                continue;
                            }

                            if let Ok(resp) = CDC_RESPONSE_CHANNEL.try_receive() {
                                match resp {
                                    CdcResponse::ScannerStatus { connected, fw_byte } => {
                                        let _ = cdc
                                            .write_packet(&[
                                                Status::Ok.to_byte(),
                                                0,
                                                3,
                                                if connected { 1 } else { 0 },
                                                1,
                                                fw_byte,
                                            ])
                                            .await;
                                    }
                                    CdcResponse::TriggerOk | CdcResponse::Ok => {
                                        let _ =
                                            cdc.write_packet(&[Status::Ok.to_byte(), 0, 0]).await;
                                    }
                                    CdcResponse::TriggerFail => {
                                        let _ = cdc
                                            .write_packet(&[
                                                Status::ScannerNotConnected.to_byte(),
                                                0,
                                                0,
                                            ])
                                            .await;
                                    }
                                    CdcResponse::ScanData { data, type_byte } => {
                                        let copy_len = data.len().min(255);
                                        let mut buf = [0u8; 256];
                                        buf[0] = type_byte;
                                        buf[1..copy_len + 1].copy_from_slice(&data[..copy_len]);
                                        let _ = cdc
                                            .write_packet(&[
                                                Status::Ok.to_byte(),
                                                0,
                                                (copy_len + 1) as u8,
                                            ])
                                            .await;
                                        let _ = cdc.write_packet(&buf[..copy_len + 1]).await;
                                    }
                                    CdcResponse::NoScanData => {
                                        let _ = cdc
                                            .write_packet(&[Status::NoScanData.to_byte(), 0, 0])
                                            .await;
                                    }
                                    CdcResponse::Settings { bits }
                                    | CdcResponse::SetSettingsResult { bits } => {
                                        let _ = cdc
                                            .write_packet(&[Status::Ok.to_byte(), 0, 1, bits])
                                            .await;
                                    }
                                    CdcResponse::CompatibilityProfile { mode } => {
                                        let _ = cdc
                                            .write_packet(&[Status::Ok.to_byte(), 0, 1, mode as u8])
                                            .await;
                                    }
                                    CdcResponse::HostOptions(profile) => {
                                        let payload = profile.serialize();
                                        let _ = cdc
                                            .write_packet(&[
                                                Status::Ok.to_byte(),
                                                0,
                                                compatibility::PROFILE_FLASH_BYTES as u8,
                                            ])
                                            .await;
                                        let _ = cdc.write_packet(&payload).await;
                                    }
                                    CdcResponse::RebootRequired => {
                                        let _ = cdc
                                            .write_packet(&[Status::RebootRequired.to_byte(), 0, 0])
                                            .await;
                                    }
                                    CdcResponse::SettingsReadFailed
                                    | CdcResponse::SetSettingsWriteFailed
                                    | CdcResponse::Error => {
                                        let _ = cdc
                                            .write_packet(&[Status::Error.to_byte(), 0, 0])
                                            .await;
                                    }
                                }
                                continue;
                            }

                            match cdc.read_packet(&mut rx_buf).await {
                                Ok(n) if n > 0 => {
                                    if let Some(frame) = frame_decoder.decode(&rx_buf[..n]) {
                                        match frame.command {
                                            Command::ScannerStatus => {
                                                let _ = COMMAND_CHANNEL
                                                    .try_send(HostCommand::ScannerStatusCdc);
                                            }
                                            Command::ScannerTrigger => {
                                                let _ =
                                                    COMMAND_CHANNEL.try_send(HostCommand::Trigger);
                                            }
                                            Command::ScannerData => {
                                                let _ = COMMAND_CHANNEL
                                                    .try_send(HostCommand::ScannerDataCdc);
                                            }
                                            Command::GetSettings => {
                                                let _ = COMMAND_CHANNEL
                                                    .try_send(HostCommand::GetSettings);
                                            }
                                            Command::SetSettings => {
                                                if let Some(&bits) = frame.payload().first() {
                                                    if let Some(settings) =
                                                        ScannerSettings::from_bits(bits)
                                                    {
                                                        let _ = COMMAND_CHANNEL.try_send(
                                                            HostCommand::SetSettings(settings),
                                                        );
                                                    } else {
                                                        let _ = cdc
                                                            .write_packet(&[
                                                                Status::InvalidPayload.to_byte(),
                                                                0,
                                                                0,
                                                            ])
                                                            .await;
                                                    }
                                                } else {
                                                    let _ = cdc
                                                        .write_packet(&[
                                                            Status::InvalidPayload.to_byte(),
                                                            0,
                                                            0,
                                                        ])
                                                        .await;
                                                }
                                            }
                                            Command::DisplayQr => {
                                                let text = core::str::from_utf8(frame.payload())
                                                    .unwrap_or("<invalid utf8>");
                                                let _ = DISPLAY_CHANNEL
                                                    .try_send(DisplayEvent::Qr(String::from(text)));
                                                let _ = cdc
                                                    .write_packet(&[Status::Ok.to_byte(), 0, 0])
                                                    .await;
                                            }
                                            Command::EnterSettings => {
                                                let _ = COMMAND_CHANNEL
                                                    .try_send(HostCommand::EnterSettings);
                                            }
                                            Command::GetCompatibilityProfile => {
                                                let _ = COMMAND_CHANNEL
                                                    .try_send(HostCommand::GetCompatibilityProfile);
                                            }
                                            Command::SetCompatibilityProfile => {
                                                if let Some(&mode_byte) = frame.payload().first() {
                                                    if let Ok(mode) =
                                                        compatibility::UsbMode::try_from(mode_byte)
                                                    {
                                                        let _ = COMMAND_CHANNEL.try_send(
                                                            HostCommand::SetCompatibilityProfile(
                                                                mode,
                                                            ),
                                                        );
                                                    } else {
                                                        let _ = cdc
                                                            .write_packet(&[
                                                                Status::InvalidPayload.to_byte(),
                                                                0,
                                                                0,
                                                            ])
                                                            .await;
                                                    }
                                                } else {
                                                    let _ = cdc
                                                        .write_packet(&[
                                                            Status::InvalidPayload.to_byte(),
                                                            0,
                                                            0,
                                                        ])
                                                        .await;
                                                }
                                            }
                                            Command::RebootUsb => {
                                                let _ = COMMAND_CHANNEL
                                                    .try_send(HostCommand::RebootUsb);
                                            }
                                            Command::GetHostOptions => {
                                                let _ = COMMAND_CHANNEL
                                                    .try_send(HostCommand::GetHostOptions);
                                            }
                                            Command::SetHostOptions => {
                                                if let Some(profile) =
                                                    compatibility::CompatibilityProfile::deserialize(
                                                        frame.payload(),
                                                    )
                                                {
                                                    let _ = COMMAND_CHANNEL.try_send(
                                                        HostCommand::SetHostOptions(profile),
                                                    );
                                                } else {
                                                    let _ = cdc
                                                        .write_packet(&[
                                                            Status::InvalidPayload.to_byte(),
                                                            0,
                                                            0,
                                                        ])
                                                        .await;
                                                }
                                            }
                                        }
                                        continue;
                                    }
                                }
                                Ok(_) => {}
                                Err(_) => break,
                            }

                            heartbeat.next().await;
                            if cdc
                                .write_packet(b"[ALIVE] gm65-scanner admin\r\n")
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Timer::after(Duration::from_millis(100)).await;
                    }
                };

                embassy_futures::join::join(usb_task, cdc_task).await;
            }
            compatibility::UsbMode::Ds2208KeyboardHid => {
                let personality = info_for_profile(active_profile);
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
                usb_config_desc.product = Some(personality.product);
                usb_config_desc.serial_number = Some(personality.serial);
                let mut config_descriptor = [0; 256];
                let mut bos_descriptor = [0; 256];
                let mut msos_descriptor = [0; 256];
                let mut control_buf = [0; 64];
                let mut request_handler = KeyboardRequestHandler;
                let mut hid_state = HidState::new();
                let mut builder = Builder::new(
                    usb_driver,
                    usb_config_desc,
                    &mut config_descriptor,
                    &mut bos_descriptor,
                    &mut msos_descriptor,
                    &mut control_buf,
                );
                let hid = HidReaderWriter::<_, 1, 8>::new(
                    &mut builder,
                    &mut hid_state,
                    HidConfig {
                        report_descriptor:
                            gm65_scanner::hid::keyboard::BOOT_KEYBOARD_REPORT_DESCRIPTOR,
                        request_handler: None,
                        poll_ms: personality.poll_ms,
                        max_packet_size: 8,
                        hid_subclass: HidSubclass::Boot,
                        hid_boot_protocol: HidBootProtocol::Keyboard,
                    },
                );
                let mut usb_dev = builder.build();
                let (reader, mut writer) = hid.split();

                let usb_task = async { usb_dev.run().await };
                let hid_out_task = async { reader.run(false, &mut request_handler).await };
                let keyboard_task = async {
                    let mut reports = heapless::Vec::<[u8; 8], 600>::new();
                    loop {
                        let result = SCAN_CHANNEL.receive().await;
                        let profile = { SHARED.lock().await.profile };
                        let caps_on = (KEYBOARD_LED_STATE.load(Ordering::Relaxed) & 0x02) != 0;
                        let stats =
                            build_keyboard_reports(profile, caps_on, &result.data, &mut reports);
                        for report in reports.iter() {
                            if writer.write(report).await.is_err() {
                                let _ = FEEDBACK_CHANNEL.try_send(FeedbackEvent::TransmissionError);
                                let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Error(
                                    String::from("USB keyboard send failed"),
                                ));
                                break;
                            }
                            if profile.key_delay_ms > 0 && report[2] == 0 {
                                Timer::after(Duration::from_millis(u64::from(
                                    profile.key_delay_ms,
                                )))
                                .await;
                            }
                        }
                        if stats.skipped_bytes > 0 {
                            log_info!(
                                "Keyboard HID: skipped {} unsupported byte(s) for this scan",
                                stats.skipped_bytes
                            );
                        }
                    }
                };

                embassy_futures::join::join(
                    usb_task,
                    embassy_futures::join::join(keyboard_task, hid_out_task),
                )
                .await;
            }
            compatibility::UsbMode::Ds2208HidPos => {
                let personality = info_for_profile(active_profile);
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
                usb_config_desc.product = Some(personality.product);
                usb_config_desc.serial_number = Some(personality.serial);
                let mut config_descriptor = [0; 256];
                let mut bos_descriptor = [0; 256];
                let mut msos_descriptor = [0; 256];
                let mut control_buf = [0; 64];
                let mut hid_state = HidState::new();
                let mut builder = Builder::new(
                    usb_driver,
                    usb_config_desc,
                    &mut config_descriptor,
                    &mut bos_descriptor,
                    &mut msos_descriptor,
                    &mut control_buf,
                );
                let mut writer = HidWriter::<_, 261>::new(
                    &mut builder,
                    &mut hid_state,
                    HidConfig {
                        report_descriptor: POS_BARCODE_SCANNER_REPORT_DESCRIPTOR,
                        request_handler: None,
                        poll_ms: personality.poll_ms,
                        max_packet_size: 64,
                        hid_subclass: HidSubclass::No,
                        hid_boot_protocol: HidBootProtocol::None,
                    },
                );
                let mut usb_dev = builder.build();
                let usb_task = async { usb_dev.run().await };
                let pos_task = async {
                    loop {
                        let result = SCAN_CHANNEL.receive().await;
                        let built = build_pos_report(&result.data, None);
                        if writer.write(&built.report.as_bytes()).await.is_err() {
                            let _ = FEEDBACK_CHANNEL.try_send(FeedbackEvent::TransmissionError);
                            let _ = DISPLAY_CHANNEL
                                .try_send(DisplayEvent::Error(String::from("USB POS send failed")));
                            continue;
                        }
                        if built.was_truncated {
                            log_info!(
                                "HID POS: truncated scan payload from {} to 256 bytes",
                                result.data.len()
                            );
                        }
                    }
                };
                embassy_futures::join::join(usb_task, pos_task).await;
            }
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
                DisplayEvent::Compatibility(profile) => {
                    crate::compat_display::render_compatibility_profile(&mut display.fb(), profile);
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
                        if point.x >= 3 && point.x <= 476 && point.y >= 3 && point.y <= 796 {
                            let _ = TOUCH_CHANNEL.try_send(TouchEvent::Tap {
                                x: point.x,
                                y: point.y,
                            });
                        }
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
                Ok(TouchEvent::Tap { y, .. }) => {
                    if y < 80 {
                        let shared = SHARED.lock().await;
                        let _ =
                            DISPLAY_CHANNEL.try_send(DisplayEvent::Compatibility(shared.profile));
                        continue;
                    }

                    let mut shared = SHARED.lock().await;
                    let Some(action) = map_touch_to_action(y) else {
                        continue;
                    };
                    let (profile, reboot) = apply_touch_action(shared.profile, action);

                    shared.profile = profile;
                    let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Compatibility(profile));
                    let _ = PROFILE_CHANNEL.try_send(ProfileCommand::Save { profile, reboot });
                    let _ = FEEDBACK_CHANNEL.try_send(FeedbackEvent::ConfigOk);
                }
                Err(_) => {
                    Timer::after(Duration::from_millis(100)).await;
                }
            }
        }
    };

    let profile_task = async move {
        loop {
            match PROFILE_CHANNEL.receive().await {
                ProfileCommand::Save { profile, reboot } => {
                    if flash_store.save(profile).await.is_err() {
                        let _ = DISPLAY_CHANNEL
                            .try_send(DisplayEvent::Error(String::from("Profile save failed")));
                        let _ = FEEDBACK_CHANNEL.try_send(FeedbackEvent::ConfigError);
                        continue;
                    }
                    let _ = DISPLAY_CHANNEL.try_send(DisplayEvent::Status(String::from(
                        profile_save_status(reboot),
                    )));
                    if reboot {
                        reboot_device_after_delay().await;
                    }
                }
            }
        }
    };

    let feedback_task = async move {
        loop {
            for pulse in pulses_for(FEEDBACK_CHANNEL.receive().await) {
                led.set_high();
                Timer::after(Duration::from_millis(u64::from(pulse.on_ms))).await;
                led.set_low();
                if pulse.off_ms > 0 {
                    Timer::after(Duration::from_millis(u64::from(pulse.off_ms))).await;
                }
            }
        }
    };

    embassy_futures::join::join(
        personality_task,
        embassy_futures::join::join(
            scanner_task,
            embassy_futures::join::join(
                display_task,
                embassy_futures::join::join(
                    embassy_futures::select::select(touch_task, settings_touch_task),
                    embassy_futures::join::join(profile_task, feedback_task),
                ),
            ),
        ),
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
mod display_async {
    const DISPLAY_CENTER_X: i32 = 240;
    const DISPLAY_MAX_Y: u32 = 800;
    include!("../display.rs");
}
mod compat_display {
    include!("../compat_display.rs");
}
mod qr_display_async {
    include!("../qr_display.rs");
}
mod cdc {
    #[cfg(feature = "scanner-async")]
    include!("../cdc.rs");
}
