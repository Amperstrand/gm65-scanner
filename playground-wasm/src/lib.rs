//! gm65-scanner browser playground — wasm rehearsal for `micronuts-web`.
//!
//! Runs the **real** async driver (`Gm65ScannerAsync`) against a virtual
//! UART wired to an in-page GM65 module simulator, driven from JavaScript:
//! camera/paste feeds decoded QR text into the module sim, which answers
//! with genuine GM65 protocol bytes (`7E 00 08 … AB CD` commands from the
//! driver, `02 00 00 01 vv 33 31` responses, `payload CRLF` scan data).
//!
//! This exercises every seam a browser wallet (micronuts-web) needs:
//! `embassy-time`'s `wasm` timer driver under the wasm-bindgen future
//! executor, a virtual `embedded-io-async` UART with waker plumbing, and
//! JS-side QR decode feeding protocol-faithful frames into the unmodified
//! driver — per the modularity rule, scanner logic stays gm65-scanner's.
//!
//! The long-term home for `Gm65ModuleSim`/`VirtualUart` is a `mock` feature
//! on the crate itself once a second consumer (micronuts-web) exists.
//!
//! Build:
//! ```text
//! cargo build -p playground-wasm --target wasm32-unknown-unknown --release
//! wasm-bindgen --target web \
//!   target/wasm32-unknown-unknown/release/playground_wasm.wasm \
//!   --out-dir web
//! python3 -m http.server -d playground-wasm/web 8901
//! ```
#![cfg(target_arch = "wasm32")]

mod lcd;
mod ui;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::future::poll_fn;
use std::rc::Rc;
use std::task::Poll;
use std::task::Waker;

use gm65_scanner::protocol::Register;
use gm65_scanner::Gm65ScannerAsync;
use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// GM65 module simulator
// ---------------------------------------------------------------------------

/// Register boot defaults; unlisted registers answer `0xFF` on GET, which
/// forces the driver's read-compare-write-verify config path (the richer
/// demo — every register gets written during `init()`).
const BOOT_REGS: [(u16, u8); 4] = [
    (0x0000, 0xD1), // Settings: datasheet default (matches bench power-up)
    (0x000D, 0xA0), // SerialOutput: already correct — init takes the no-fix path
    (0x0002, 0x00), // ScanEnable: stopped
    (0x00E2, 0x87), // Version: firmware 0x87 (no raw-mode fix needed)
];

/// In-page GM65 module: parses host command frames and responds with real
/// protocol bytes. Scan data (QR payload + CRLF) is injected from JS.
pub struct Gm65ModuleSim {
    regs: BTreeMap<u16, u8>,
    frame: Vec<u8>,
    scanning: bool,
    pending_scan: Option<Vec<u8>>,
}

impl Gm65ModuleSim {
    fn new() -> Self {
        Self {
            regs: BOOT_REGS.iter().copied().collect(),
            frame: Vec::new(),
            scanning: false,
            pending_scan: None,
        }
    }

    fn reg_get(&self, addr: u16) -> u8 {
        self.regs.get(&addr).copied().unwrap_or(0xFF)
    }

    fn settings_bits(&self) -> u8 {
        self.regs.get(&0x0000).copied().unwrap_or(0xFF)
    }

    fn ack(value: u8) -> Vec<u8> {
        vec![0x02, 0x00, 0x00, 0x01, value, 0x33, 0x31]
    }

    /// Feed one host byte; returns the module's output bytes for it.
    fn feed_byte(&mut self, b: u8) -> Vec<u8> {
        // Resync: ignore bytes outside a frame.
        if self.frame.is_empty() && b != 0x7E {
            return Vec::new();
        }
        self.frame.push(b);
        if self.frame.len() == 2 && b != 0x00 {
            self.frame.clear();
            return Vec::new();
        }
        if self.frame.len() < 4 {
            return Vec::new();
        }
        let total = 4 + self.frame[3] as usize + 2;
        if self.frame.len() < total {
            return Vec::new();
        }
        let frame = core::mem::take(&mut self.frame);
        let mut out = Vec::new();
        let addr = u16::from_be_bytes([frame[4], frame[5]]);
        let value = frame.get(6).copied().unwrap_or(0);
        match frame[2] {
            0x07 => out.extend(Self::ack(self.reg_get(addr))),
            0x08 => {
                if addr == Register::ScanEnable as u16 {
                    self.scanning = value == 0x01;
                } else if addr != Register::BarType as u16 {
                    // BarType: ACKed but not persisted — the firmware-0.87
                    // quirk documented on the Register enum.
                    self.regs.insert(addr, value);
                }
                // Real module order: ACK the trigger first, then stream the
                // decode (if one is pending) after it.
                out.extend(Self::ack(value));
                if self.scanning {
                    if let Some(p) = self.pending_scan.take() {
                        out.extend(p);
                    }
                } else {
                    self.pending_scan = None;
                }
            }
            0x09 => out.extend(Self::ack(0x00)),
            _ => {}
        }
        out
    }

    /// Queue a decoded QR payload (JS camera or paste). Emitted as
    /// `payload CRLF` once the module scans, else held pending.
    fn inject_scan(&mut self, payload: &str) -> Vec<u8> {
        let mut data = payload.as_bytes().to_vec();
        data.extend_from_slice(b"\r\n");
        if self.scanning {
            data
        } else {
            self.pending_scan = Some(data);
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Virtual UART
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UartError;

impl std::fmt::Display for UartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "virtual uart error")
    }
}

impl std::error::Error for UartError {}

impl embedded_io_async::Error for UartError {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        embedded_io_async::ErrorKind::Other
    }
}

struct VuartInner {
    /// Bytes waiting for the driver's next `read` (module → host).
    to_driver: VecDeque<u8>,
    read_waker: Option<Waker>,
    module: Gm65ModuleSim,
    /// Host → module byte history (driver TX), for the UART monitor.
    tx_log: Vec<u8>,
    /// Module → host byte history (driver RX), for the UART monitor.
    rx_log: Vec<u8>,
}

/// A virtual UART shared between the driver and the module simulator.
///
/// Writes are answered synchronously inside `write()` (the sim parses the
/// command and queues its response), so command/response rounds never touch
/// the waker path. The waker path is exercised by scan data injected from
/// JS while the driver is awaiting `read()`.
#[derive(Clone)]
pub struct VirtualUart(Rc<RefCell<VuartInner>>);

impl VirtualUart {
    fn new() -> Self {
        Self(Rc::new(RefCell::new(VuartInner {
            to_driver: VecDeque::new(),
            read_waker: None,
            module: Gm65ModuleSim::new(),
            tx_log: Vec::new(),
            rx_log: Vec::new(),
        })))
    }

    /// Push module output bytes and wake any pending driver read.
    fn push_module_bytes(&self, bytes: &[u8]) {
        let waker = {
            let mut inner = self.0.borrow_mut();
            for b in bytes {
                inner.to_driver.push_back(*b);
                inner.rx_log.push(*b);
            }
            inner.read_waker.take()
        };
        if let Some(w) = waker {
            w.wake();
        }
    }

    /// JS entry: a QR code was decoded (camera or paste).
    fn inject_scan(&self, payload: &str) {
        let out = self.0.borrow_mut().module.inject_scan(payload);
        if !out.is_empty() {
            self.push_module_bytes(&out);
        }
    }

    fn is_scanning(&self) -> bool {
        self.0.borrow().module.scanning
    }

    /// The module's live SETTINGS register (boot value 0xD1).
    fn module_settings(&self) -> u8 {
        self.0.borrow().module.settings_bits()
    }

    fn log_hex(log: &[u8]) -> String {
        const CAP: usize = 256;
        let start = log.len().saturating_sub(CAP);
        let mut s = String::new();
        for b in &log[start..] {
            s.push_str(&format!("{b:02X} "));
        }
        s
    }
}

impl embedded_io_async::ErrorType for VirtualUart {
    type Error = UartError;
}

impl embedded_io_async::Write for VirtualUart {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let mut responses = Vec::new();
        {
            let mut inner = self.0.borrow_mut();
            for b in buf {
                inner.tx_log.push(*b);
                responses.extend(inner.module.feed_byte(*b));
            }
        }
        if !responses.is_empty() {
            self.push_module_bytes(&responses);
        }
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl embedded_io_async::Read for VirtualUart {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        poll_fn(move |cx| {
            let mut inner = self.0.borrow_mut();
            if inner.to_driver.is_empty() {
                inner.read_waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
            let mut n = 0;
            while n < buf.len() {
                match inner.to_driver.pop_front() {
                    Some(b) => buf[n] = b,
                    None => break,
                }
                n += 1;
            }
            Poll::Ready(Ok(n))
        })
        .await
    }
}

// ---------------------------------------------------------------------------
// JS exports
//
// Architecture (validated against chip-agnostic/sans-IO practice):
// one device task owns the driver — the embassy "task owns peripheral"
// pattern; every other entry point (wing buttons, LCD taps, camera decodes)
// sends commands through woken queues, and state leaves the task only via
// event callbacks and mirrors. The UART handle is Rc-shared so scan
// injection never serializes behind driver ops (injecting mid-read is the
// camera path).
// ---------------------------------------------------------------------------

thread_local! {
    static UART: RefCell<Option<VirtualUart>> = const { RefCell::new(None) };
    static LCD: RefCell<lcd::Lcd> = RefCell::new(lcd::Lcd::new());
}

fn with_uart<T>(f: impl FnOnce(&VirtualUart) -> T) -> Option<T> {
    UART.with(|u| u.borrow().clone()).map(|uart| f(&uart))
}

#[wasm_bindgen]
pub fn pg_boot() {
    console_error_panic_hook::set_once();
    ui::boot_render();
}

/// Install the JS event sink: `{"type":"log"|"scan"|"screen", ...}`.
#[wasm_bindgen]
pub fn pg_set_on_event(cb: js_sys::Function) {
    ui::set_on_event(cb);
}

/// Create the device task: virtual module + real driver, spawn the loop.
#[wasm_bindgen]
pub async fn pg_init() -> Result<JsValue, JsValue> {
    ui::reset_state();
    let uart = VirtualUart::new();
    UART.with(|u| *u.borrow_mut() = Some(uart.clone()));
    let scanner = Gm65ScannerAsync::with_default_config(uart.clone());
    wasm_bindgen_futures::spawn_local(ui::device_task(scanner, uart));
    Ok(JsValue::from_str("starting"))
}

/// Host wing: start scanning with policy 0/1/2 (see ScanPolicy).
#[wasm_bindgen]
pub fn pg_cmd_start(policy: u32) {
    ui::POLICY_IDX.with(|p| p.set(policy));
    ui::push_cmd(ui::UiCmd::StartScan(policy));
}

/// Host wing: stop scanning.
#[wasm_bindgen]
pub fn pg_cmd_stop() {
    ui::push_cmd(ui::UiCmd::StopScan);
}

/// JS entry for decoded QR text (camera or paste). Delivered as
/// `payload CRLF` by the module when it is scanning.
#[wasm_bindgen]
pub fn pg_feed_scan_payload(payload: &str) {
    if let Some(u) = with_uart(|u| u.clone()) {
        u.inject_scan(payload);
    }
}

/// LCD tap in 480x800 pixel coordinates (JS maps CSS → pixels).
#[wasm_bindgen]
pub fn pg_lcd_tap(x: u32, y: u32) {
    ui::push_tap(x, y);
}

/// Current screen: boot | home | scanning | result | settings.
#[wasm_bindgen]
pub fn pg_state() -> String {
    ui::UI_STATE.with(|s| s.borrow().screen.name()).to_string()
}

/// Mirror of device status for the wing panel.
#[wasm_bindgen]
pub fn pg_status() -> String {
    let screen = pg_state();
    let model = ui::UI_STATE.with(|s| s.borrow().model.clone());
    let scanning = pg_module_scanning();
    let bits = pg_module_settings();
    format!(
        "screen: {screen} · model: {model} · module scanning: {scanning} · SETTINGS: 0x{bits:02X}"
    )
}

/// True while the virtual module's scan laser is on.
#[wasm_bindgen]
pub fn pg_module_scanning() -> bool {
    with_uart(|u| u.is_scanning()).unwrap_or(false)
}

/// The module's live SETTINGS register value.
#[wasm_bindgen]
pub fn pg_module_settings() -> u8 {
    with_uart(|u| u.module_settings()).unwrap_or(0xFF)
}

/// Touch targets of the current screen (id, y, h in 480x800 pixels) for
/// the e2e harness and accessibility tooling.
#[wasm_bindgen]
pub fn pg_ui_tap_targets() -> JsValue {
    let screen = ui::UI_STATE.with(|s| s.borrow().screen);
    let mut json = format!("{{\"screen\":\"{}\",\"rows\":[", screen.name());
    for (i, (id, y)) in ui::rows_for(screen).iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            "{{\"id\":\"{}\",\"y\":{},\"h\":{}}}",
            id.name(),
            y,
            lcd::ROW_H
        ));
    }
    json.push_str("]}");
    JsValue::from_str(&json)
}

/// Last driver TX bytes (host → module commands), hex.
#[wasm_bindgen]
pub fn pg_tx_hex() -> String {
    with_uart(|u| VirtualUart::log_hex(&u.0.borrow().tx_log)).unwrap_or_default()
}

/// Last driver RX bytes (module → host responses + scan data), hex.
#[wasm_bindgen]
pub fn pg_rx_hex() -> String {
    with_uart(|u| VirtualUart::log_hex(&u.0.borrow().rx_log)).unwrap_or_default()
}
