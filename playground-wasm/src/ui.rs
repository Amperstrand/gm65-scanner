//! Device UI task — the firmware-shaped heart of the playground.
//!
//! One task owns the driver; everything else (host wing buttons, LCD taps,
//! camera decodes) reaches it through the shared queues below. The LCD now
//! reacts to scans by itself: while the module is scanning, a standing
//! `read_scan` is always pending, so any decode (camera or paste) lands on
//! the display immediately — mirroring the firmware's poll loop instead of
//! waiting for an explicit host Read.
//!
//! Interaction model mirrors micronuts-app: polled touch rows on a fixed
//! 480x800 layout, hit-tested in Rust. Hand-rolled rows (not a GUI crate)
//! on purpose — firmware parity is the point.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::poll_fn;
use std::rc::Rc;
use std::task::Poll;
use std::task::Waker;

use gm65_scanner::settings::{AimSetting, LightSetting, ReadMode, ScannerSettings};
use gm65_scanner::{Gm65ScannerAsync, ScanPolicy, ScannerDriver};
use wasm_bindgen::prelude::*;

use crate::lcd;
use crate::VirtualUart;
use crate::LCD;

// ---------------------------------------------------------------------------
// Shared queues (single-threaded, waker-driven)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Boot,
    Home,
    Scanning,
    Result,
    Settings,
}

impl Screen {
    pub fn name(self) -> &'static str {
        match self {
            Screen::Boot => "boot",
            Screen::Home => "home",
            Screen::Scanning => "scanning",
            Screen::Result => "result",
            Screen::Settings => "settings",
        }
    }
}

pub enum UiCmd {
    StartScan(u32),
    StopScan,
    ApplySettings(ScannerSettings),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowId {
    Start,
    Stop,
    SettingsNav,
    Back,
    Buzzer,
    Aim,
    Light,
    Mode,
}

impl RowId {
    pub fn name(self) -> &'static str {
        match self {
            RowId::Start => "start",
            RowId::Stop => "stop",
            RowId::SettingsNav => "settings",
            RowId::Back => "back",
            RowId::Buzzer => "buzzer",
            RowId::Aim => "aim",
            RowId::Light => "light",
            RowId::Mode => "mode",
        }
    }
}

struct Shared {
    cmds: VecDeque<UiCmd>,
    taps: VecDeque<(u32, u32)>,
    waker: Option<Waker>,
}

impl Shared {
    fn wake(&mut self) {
        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }
}

thread_local! {
    static SHARED: RefCell<Rc<RefCell<Shared>>> = RefCell::new(Rc::new(RefCell::new(Shared {
        cmds: VecDeque::new(),
        taps: VecDeque::new(),
        waker: None,
    })));
    pub static ON_EVENT: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    pub static UI_STATE: RefCell<UiState> = const { RefCell::new(UiState::new()) };
    pub static POLICY_IDX: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[derive(Debug, Clone)]
pub struct UiState {
    pub screen: Screen,
    pub model: String,
    pub payload: Option<String>,
    pub policy_name: &'static str,
    pub last_error: Option<String>,
}

impl UiState {
    const fn new() -> Self {
        Self {
            screen: Screen::Boot,
            model: String::new(),
            payload: None,
            policy_name: "SilentContinuous",
            last_error: None,
        }
    }
}

fn with_shared<T>(f: impl FnOnce(&mut Shared) -> T) -> T {
    SHARED.with(|s| {
        let shared = s.borrow().clone();
        let mut inner = shared.borrow_mut();
        f(&mut inner)
    })
}

pub fn push_cmd(cmd: UiCmd) {
    with_shared(|s| {
        s.cmds.push_back(cmd);
        s.wake();
    });
}

pub fn push_tap(x: u32, y: u32) {
    with_shared(|s| {
        s.taps.push_back((x, y));
        s.wake();
    });
}

async fn wait_for_input() {
    poll_fn(|cx| {
        with_shared(|s| {
            if s.cmds.is_empty() && s.taps.is_empty() {
                s.waker = Some(cx.waker().clone());
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
    })
    .await
}

pub fn set_on_event(cb: js_sys::Function) {
    ON_EVENT.with(|slot| *slot.borrow_mut() = Some(cb));
}

/// Fresh state for a new device task (re-Init).
pub fn reset_state() {
    UI_STATE.with(|s| *s.borrow_mut() = UiState::new());
}

fn emit(kind: &str, key: &str, value: &str) {
    let json = format!("{{\"type\":\"{kind}\",\"{key}\":\"{value}\"}}");
    ON_EVENT.with(|slot| {
        if let Some(cb) = slot.borrow().as_ref() {
            let _ = cb.call1(&JsValue::NULL, &JsValue::from_str(&json));
        }
    });
}

// ---------------------------------------------------------------------------
// Touch row layout (480x800)
// ---------------------------------------------------------------------------

// Touch-row hit-testing uses the same ROW_X/ROW_W/ROW_H constants the
// renderer draws with (defined in lcd.rs) — layout and hit-test cannot
// drift apart.

/// y positions of the touch rows per screen (row tops).
const ROWS_HOME: [(RowId, i32); 2] = [(RowId::Start, 150), (RowId::SettingsNav, 240)];
const ROWS_SCANNING: [(RowId, i32); 1] = [(RowId::Stop, 150)];
const ROWS_RESULT: [(RowId, i32); 1] = [(RowId::Back, 726)];
const ROWS_SETTINGS: [(RowId, i32); 5] = [
    (RowId::Buzzer, 130),
    (RowId::Aim, 210),
    (RowId::Light, 290),
    (RowId::Mode, 370),
    (RowId::Back, 680),
];

pub fn rows_for(screen: Screen) -> &'static [(RowId, i32)] {
    match screen {
        Screen::Home => &ROWS_HOME,
        Screen::Scanning => &ROWS_SCANNING,
        Screen::Result => &ROWS_RESULT,
        Screen::Settings => &ROWS_SETTINGS,
        Screen::Boot => &[],
    }
}

fn hit_test(screen: Screen, x: u32, y: u32) -> Option<RowId> {
    let (x, y) = (x as i32, y as i32);
    rows_for(screen)
        .iter()
        .find(|(_, ry)| {
            y >= *ry && y < *ry + lcd::ROW_H && (lcd::ROW_X..=lcd::ROW_X + lcd::ROW_W).contains(&x)
        })
        .map(|(id, _)| *id)
}

// ---------------------------------------------------------------------------
// The device task
// ---------------------------------------------------------------------------

pub async fn device_task(mut scanner: Gm65ScannerAsync<VirtualUart>, uart: VirtualUart) {
    let model = scanner.init().await;
    match model {
        Ok(m) => {
            let name = m.to_string();
            UI_STATE.with(|s| {
                s.borrow_mut().model = name.clone();
                s.borrow_mut().screen = Screen::Home;
            });
            emit("log", "msg", &format!("init ✓ model={name}"));
        }
        Err(e) => {
            UI_STATE.with(|s| {
                s.borrow_mut().last_error = Some(format!("{e}"));
            });
            emit("log", "msg", &format!("init ✗ {e}"));
        }
    }
    render(&uart);
    emit("screen", "screen", current_screen().name());

    loop {
        // 1. Drain taps → commands/screen transitions.
        while let Some((x, y)) = with_shared(|s| s.taps.pop_front()) {
            let screen = current_screen();
            if let Some(row) = hit_test(screen, x, y) {
                handle_tap(row, &uart);
            }
        }

        // 2. Drain commands (from taps and the host wing).
        while let Some(cmd) = with_shared(|s| s.cmds.pop_front()) {
            apply_cmd(cmd, &mut scanner, &uart).await;
        }

        // 3. Standing read while scanning — the wiring fix: any decode
        //    reaches the LCD without a host Read, like the firmware poll.
        let scanning = uart.is_scanning();
        if scanning {
            let res = embassy_time::with_timeout(
                embassy_time::Duration::from_millis(300),
                scanner.read_scan(),
            )
            .await;
            if let Ok(Some(data)) = res {
                let text = String::from_utf8_lossy(&data).into_owned();
                UI_STATE.with(|s| {
                    s.borrow_mut().payload = Some(text.clone());
                    s.borrow_mut().screen = Screen::Result;
                });
                emit("scan", "payload", &text);
                render(&uart);
            }
        } else {
            let _ = embassy_time::with_timeout(
                embassy_time::Duration::from_millis(250),
                wait_for_input(),
            )
            .await;
        }
    }
}

fn current_screen() -> Screen {
    UI_STATE.with(|s| s.borrow().screen)
}

fn set_screen(screen: Screen, uart: &VirtualUart) {
    UI_STATE.with(|s| s.borrow_mut().screen = screen);
    render(uart);
    emit("screen", "screen", screen.name());
}

fn handle_tap(row: RowId, uart: &VirtualUart) {
    match row {
        RowId::Start => push_cmd(UiCmd::StartScan(POLICY_IDX.with(|p| p.get()))),
        RowId::Stop => push_cmd(UiCmd::StopScan),
        RowId::SettingsNav => set_screen(Screen::Settings, uart),
        RowId::Back => set_screen(Screen::Home, uart),
        RowId::Buzzer | RowId::Aim | RowId::Light | RowId::Mode => {
            let mut s = ScannerSettings::from_bits(uart.module_settings());
            match row {
                RowId::Buzzer => s.buzzer = !s.buzzer,
                RowId::Aim => {
                    s.aim = match s.aim {
                        AimSetting::Off => AimSetting::Reading,
                        AimSetting::Reading => AimSetting::Always,
                        AimSetting::Always => AimSetting::Off,
                    }
                }
                RowId::Light => {
                    s.light = match s.light {
                        LightSetting::Off => LightSetting::Reading,
                        LightSetting::Reading => LightSetting::Always,
                        LightSetting::Always => LightSetting::Off,
                    }
                }
                RowId::Mode => {
                    s.read_mode = match s.read_mode {
                        ReadMode::Manual => ReadMode::Command,
                        ReadMode::Command => ReadMode::Continuous,
                        ReadMode::Continuous => ReadMode::Induction,
                        ReadMode::Induction => ReadMode::Manual,
                    }
                }
                _ => unreachable!(),
            }
            push_cmd(UiCmd::ApplySettings(s));
        }
    }
}

async fn apply_cmd(cmd: UiCmd, scanner: &mut Gm65ScannerAsync<VirtualUart>, uart: &VirtualUart) {
    match cmd {
        UiCmd::StartScan(idx) => {
            let (policy, name) = match idx {
                0 => (ScanPolicy::SilentContinuous, "SilentContinuous"),
                1 => (ScanPolicy::BuzzingContinuous, "BuzzingContinuous"),
                _ => (ScanPolicy::SilentCommand, "SilentCommand"),
            };
            match scanner.start_scanning(policy).await {
                Ok(()) => {
                    UI_STATE.with(|s| s.borrow_mut().policy_name = name);
                    emit("log", "msg", "scanning started");
                    set_screen(Screen::Scanning, uart);
                }
                Err(e) => emit("log", "msg", &format!("start_scanning ✗ {e}")),
            }
        }
        UiCmd::StopScan => {
            if scanner.stop_scan().await {
                emit("log", "msg", "scan stopped");
                set_screen(Screen::Home, uart);
            }
        }
        // Dry settings: written through the real driver as a genuine
        // SETTINGS register frame — only the virtual module is affected.
        UiCmd::ApplySettings(s) => {
            if scanner.set_scanner_settings(s).await {
                emit(
                    "log",
                    "msg",
                    &format!("settings → 0x{:02X} (dry)", uart.module_settings()),
                );
            } else {
                emit("log", "msg", "settings write ✗");
            }
            if current_screen() == Screen::Settings {
                render(uart);
            }
        }
    }
}

fn render(uart: &VirtualUart) {
    let bits = uart.module_settings();
    let settings = ScannerSettings::from_bits(bits);
    UI_STATE.with(|s| {
        let state = s.borrow();
        LCD.with(|l| l.borrow_mut().render(&state, &settings, bits));
    });
}

pub fn boot_render() {
    UI_STATE.with(|s| {
        let state = s.borrow();
        LCD.with(|l| {
            l.borrow_mut()
                .render(&state, &ScannerSettings::default(), 0)
        });
    });
}
