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

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::future::poll_fn;
use std::rc::Rc;
use std::task::Poll;
use std::task::Waker;

use gm65_scanner::protocol::Register;
use gm65_scanner::{Gm65ScannerAsync, ScanPolicy, ScannerDriver};
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
// Session + JS exports
// ---------------------------------------------------------------------------

struct Session {
    scanner: Gm65ScannerAsync<VirtualUart>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    // JS event entry points (scan inject, UART monitors) must never be
    // serialized behind driver ops: an async export takes the Session out
    // for the whole await, so these use an independent handle into the
    // Rc-shared UART instead. Injecting mid-read is the whole point — the
    // camera path depends on it.
    static UART: RefCell<Option<VirtualUart>> = const { RefCell::new(None) };
    static IN_FLIGHT: Cell<bool> = const { Cell::new(false) };
    static LCD: RefCell<lcd::Lcd> = RefCell::new(lcd::Lcd::new());
    static MODEL: RefCell<String> = const { RefCell::new(String::new()) };
}

fn take_session() -> Result<Session, JsValue> {
    let sess = SESSION
        .with(|s| s.borrow_mut().take())
        .ok_or_else(|| JsValue::from_str("scanner not initialized — run Init first"));
    if sess.is_ok() {
        IN_FLIGHT.with(|b| b.set(true));
    }
    sess
}

fn restore_session(sess: Session) {
    SESSION.with(|s| *s.borrow_mut() = Some(sess));
    IN_FLIGHT.with(|b| b.set(false));
}

fn with_uart<T>(f: impl FnOnce(&VirtualUart) -> T) -> Option<T> {
    UART.with(|u| u.borrow().clone()).map(|uart| f(&uart))
}

#[wasm_bindgen]
pub fn pg_boot() {
    console_error_panic_hook::set_once();
    LCD.with(|l| l.borrow_mut().boot());
}

/// Initialize the scanner against the virtual module. Resolves with the
/// detected model name.
#[wasm_bindgen]
pub async fn pg_init() -> Result<JsValue, JsValue> {
    let uart = VirtualUart::new();
    let mut scanner = Gm65ScannerAsync::with_default_config(uart.clone());
    let model = scanner.init().await;
    match model {
        Ok(m) => {
            let name = m.to_string();
            MODEL.with(|slot| *slot.borrow_mut() = name.clone());
            UART.with(|u| *u.borrow_mut() = Some(uart));
            SESSION.with(|s| *s.borrow_mut() = Some(Session { scanner }));
            LCD.with(|l| l.borrow_mut().home(&name));
            Ok(JsValue::from_str(&name))
        }
        Err(e) => Err(JsValue::from_str(&format!("init failed: {e}"))),
    }
}

/// Apply a blessed scan policy: 0 = SilentContinuous, 1 = BuzzingContinuous,
/// 2 = SilentCommand.
#[wasm_bindgen]
pub async fn pg_start_scanning(policy: u32) -> Result<JsValue, JsValue> {
    let (policy, name) = match policy {
        0 => (ScanPolicy::SilentContinuous, "SilentContinuous"),
        1 => (ScanPolicy::BuzzingContinuous, "BuzzingContinuous"),
        _ => (ScanPolicy::SilentCommand, "SilentCommand"),
    };
    let mut sess = take_session()?;
    let res = sess.scanner.start_scanning(policy).await;
    restore_session(sess);
    match res {
        Ok(()) => {
            LCD.with(|l| l.borrow_mut().scanning(name));
            Ok(JsValue::from_str("scanning started"))
        }
        Err(e) => Err(JsValue::from_str(&format!("start_scanning failed: {e}"))),
    }
}

/// Await one scan for up to `timeout_ms`. Resolves with the decoded payload
/// text, or null on timeout.
#[wasm_bindgen]
pub async fn pg_read_scan(timeout_ms: u32) -> JsValue {
    let sess = match take_session() {
        Ok(s) => s,
        Err(_) => return JsValue::NULL,
    };
    let (sess, res) = {
        let mut sess = sess;
        let dur = embassy_time::Duration::from_millis(timeout_ms as u64);
        let res = embassy_time::with_timeout(dur, sess.scanner.read_scan()).await;
        (sess, res)
    };
    restore_session(sess);
    match res {
        Ok(Some(data)) => {
            let text = String::from_utf8_lossy(&data).into_owned();
            LCD.with(|l| l.borrow_mut().result(&text));
            JsValue::from_str(&text)
        }
        _ => JsValue::NULL,
    }
}

/// Stop an ongoing scan.
#[wasm_bindgen]
pub async fn pg_stop_scan() -> JsValue {
    let sess = match take_session() {
        Ok(s) => s,
        Err(_) => return JsValue::from_bool(false),
    };
    let (sess, ok) = {
        let mut sess = sess;
        let ok = sess.scanner.stop_scan().await;
        (sess, ok)
    };
    restore_session(sess);
    if ok {
        let model = MODEL.with(|m| m.borrow().clone());
        LCD.with(|l| l.borrow_mut().home(&model));
    }
    JsValue::from_bool(ok)
}

/// Current driver state (Debug format), e.g. `Ready` / `Scanning`.
#[wasm_bindgen]
pub fn pg_state() -> String {
    let in_flight = IN_FLIGHT.with(|b| b.get());
    SESSION.with(|s| match s.borrow().as_ref() {
        Some(sess) => format!("{:?}", sess.scanner.state()),
        None if in_flight => "busy".to_string(),
        None => "uninitialized".to_string(),
    })
}

/// Driver status (model, connected, last scan length) as Debug text.
#[wasm_bindgen]
pub fn pg_status() -> String {
    SESSION.with(|s| match s.borrow().as_ref() {
        Some(sess) => format!("{:?}", sess.scanner.status()),
        None => String::new(),
    })
}

/// True while the virtual module's scan laser is on.
#[wasm_bindgen]
pub fn pg_module_scanning() -> bool {
    with_uart(|u| u.is_scanning()).unwrap_or(false)
}

/// JS entry for decoded QR text (camera or paste). Held pending until the
/// module is scanning, then delivered as `payload CRLF`.
#[wasm_bindgen]
pub fn pg_feed_scan_payload(payload: &str) {
    if let Some(u) = with_uart(|u| u.clone()) {
        u.inject_scan(payload);
    }
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
