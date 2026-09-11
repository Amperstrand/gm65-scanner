//! CYD QR-display server — bench QR source for gm65-scanner HIL testing.
//!
//! Hardware: ESP32-2432S028 "Cheap Yellow Display" (ESP32-D0WD-V3,
//! ILI9341 240x320 over SPI, CH340 USB-serial on UART0).
//! Pin map from the Amperstrand esphome fork (components/mipi_spi/models/cyd.py):
//!   ILI9341: SCK=GPIO14, MOSI=GPIO13, CS=GPIO15, DC=GPIO2, backlight=GPIO21
//!   Bus mates held inactive: XPT2046 touch CS=GPIO33, SD CS=GPIO5
//!
//! UART0 line protocol (115200 8N1, ASCII, '\n'-terminated):
//!   `QR <hex-payload>\n` -> render QR centered, quiet zone 4 modules
//!                           reply: `RENDERED <modules> <px-per-module> <payload-len>\n`
//!   `CLR\n`               -> white screen,        reply: `CLEARED\n`
//!   `ID\n`                -> identify,            reply: `CYDQR 1.0.0\n`
//!   anything else         ->                      reply: `ERR UNKNOWN|TOOLONG|BADHEX|NOTUTF8|QRFAIL\n`
//!
//! Payloads are hex-encoded (8-bit clean) and must be valid UTF-8 (the QR
//! encoder used here is text-mode; test payloads are ASCII). Max payload
//! 250 bytes on every form (`QR`/`QRS`/`QRP`) — LINE_MAX is sized for the
//! worst line, `QRP <x> <y> <hex>`.

#![no_std]
#![no_main]

esp_bootloader_esp_idf::esp_app_desc!();

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::Rate;
use esp_hal::uart::{Config as UartConfig, Uart};
use embedded_io_async::Write as _;
use mipidsi::interface::SpiInterface;
use mipidsi::options::{ColorOrder, Orientation};
use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};

const FW_ID: &str = "CYDQR 1.0.0";

// Screen geometry (portrait)
// Panel config: this bench unit is the 320x480 ST7796 "CYD" (pin map from
// the working slint demo in cyk/embassy-hello: ST7796, inverted, BGR,
// BL=GPIO27 active-high, SPI@10MHz SCK14/MOSI13/MISO12, CS15/DC2).
// The other features remain for the 240x320 ESP32-2432S028 CYD family
// (esphome fork models/cyd.py) if one lands on the bench.
#[cfg(feature = "st7796")]
const SCREEN_W: usize = 320;
#[cfg(feature = "st7796")]
const SCREEN_H: usize = 480;
#[cfg(feature = "st7796")]
const QR_USABLE_PX: usize = 288;
#[cfg(feature = "st7796")]
const SPI_MHZ: u32 = 10;
#[cfg(not(feature = "st7796"))]
const SPI_MHZ: u32 = 16;
#[cfg(all(not(feature = "st7796"), feature = "ili9342"))]
const SCREEN_W: usize = 320; // ILI9342C framebuffer is 320x240 (landscape)
#[cfg(all(not(feature = "st7796"), feature = "ili9342"))]
const SCREEN_H: usize = 240;
#[cfg(all(not(feature = "st7796"), not(feature = "ili9342")))]
const SCREEN_W: usize = 240;
#[cfg(all(not(feature = "st7796"), not(feature = "ili9342")))]
const SCREEN_H: usize = 320;
#[cfg(not(feature = "st7796"))]
const QR_USABLE_PX: usize = 224;
const QUIET_MODULES: usize = 4; // QR spec quiet zone

// Payload limits (hex on the wire)
const PAYLOAD_MAX: usize = 250;
// Worst command line: "QRP 480 320 " (12 chars) + 2*PAYLOAD_MAX hex —
// "QRS 224 " (8) and "QR " (3) are shorter. The old 3 + 2*PAYLOAD_MAX
// rejected the 250B envelope cell via QRS (bench 2026-09-10, #94).
const LINE_MAX: usize = 12 + 2 * PAYLOAD_MAX;

// QR scratch buffers sized for the max version the payloads can reach
const QR_BUF: usize = Version::MAX.buffer_len();

type Display = mipidsi::Display<
    SpiInterface<
        'static,
        ExclusiveDevice<Spi<'static, esp_hal::Blocking>, Output<'static>, Delay>,
        Output<'static>,
    >,
    PanelModel,
    mipidsi::NoResetPin,
>;

// CYD variants ship different panels (esphome fork models/cyd.py + the
// slint demo in cyk/embassy-hello); pick by cargo feature — SDO is unwired
// on this board so runtime ID reads fail.
#[cfg(all(feature = "ili9341", not(any(feature = "st7789", feature = "ili9342", feature = "st7796"))))]
type PanelModel = mipidsi::models::ILI9341Rgb565;
#[cfg(all(feature = "st7789", not(any(feature = "ili9341", feature = "ili9342", feature = "st7796"))))]
type PanelModel = mipidsi::models::ST7789;
#[cfg(all(feature = "ili9342", not(any(feature = "ili9341", feature = "st7789", feature = "st7796"))))]
type PanelModel = mipidsi::models::ILI9342CRgb565;
#[cfg(all(feature = "st7796", not(any(feature = "ili9341", feature = "st7789", feature = "ili9342"))))]
type PanelModel = mipidsi::models::ST7796;

type Tx = esp_hal::uart::UartTx<'static, esp_hal::Async>;

static ECC_HIGH: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

const WHITE: Rgb565 = Rgb565::WHITE;
const BLACK: Rgb565 = Rgb565::BLACK;

const HEX: [u8; 16] = *b"0123456789abcdef";

fn read_panel_ids(
    spi: &mut Spi<'static, esp_hal::Blocking>,
    dc: &mut Output<'static>,
    cs: &mut Output<'static>,
) -> [[u8; 4]; 3] {
    let mut out = [[0u8; 4]; 3];
    for (i, reg) in [0x04u8, 0xD3, 0xBF].into_iter().enumerate() {
        cs.set_low();
        dc.set_low();
        let _ = spi.write(&[reg]);
        dc.set_high();
        let _ = spi.read(&mut out[i]);
        cs.set_high();
    }
    out
}

fn read_xpt2046(
    spi: &mut Spi<'static, esp_hal::Blocking>,
    cs: &mut Output<'static>,
) -> [u16; 4] {
    let mut out = [0u16; 4];
    for (i, ctrl) in [0xB0u8, 0xC0, 0x90, 0xD0].into_iter().enumerate() {
        cs.set_low();
        let _ = spi.write(&[ctrl]);
        let mut raw = [0u8; 2];
        let _ = spi.read(&mut raw);
        cs.set_high();
        out[i] = (((raw[0] as u16) << 8) | raw[1] as u16) >> 3 & 0x0FFF;
    }
    out
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = esp_hal::init(esp_hal::Config::default());

    // esp-rtos heartbeat/timer plumbing (microfips-esp32 pattern)
    let sw_ints = esp_hal::interrupt::software::SoftwareInterruptControl::new(p.SW_INTERRUPT);
    let timg0 = esp_hal::timer::timg::TimerGroup::new(p.TIMG0);
    esp_rtos::start(timg0.timer0, sw_ints.software_interrupt0);

    // Backlight on; keep the display's SPI bus mates deselected.
    #[cfg(all(feature = "bl27", not(feature = "bl_low")))]
    let _backlight = Output::new(p.GPIO27, Level::High, OutputConfig::default());
    #[cfg(all(feature = "bl27", feature = "bl_low"))]
    let _backlight = Output::new(p.GPIO27, Level::Low, OutputConfig::default());
    #[cfg(all(not(feature = "bl27"), not(feature = "bl_low")))]
    let _backlight = Output::new(p.GPIO21, Level::High, OutputConfig::default());
    #[cfg(all(not(feature = "bl27"), feature = "bl_low"))]
    let _backlight = Output::new(p.GPIO21, Level::Low, OutputConfig::default());
    let mut xpt_cs = Output::new(p.GPIO33, Level::High, OutputConfig::default());
    let _sd_cs = Output::new(p.GPIO5, Level::High, OutputConfig::default());

    // ILI9341 over SPI2 @ 40 MHz mode 0
    // 16 MHz: panel SDO reads are unreliable faster (ILI9341 serial read limit)
    // — the ST7796 unit runs at the slint demo's proven 10 MHz (SPI_MHZ).
    let mut spi = Spi::<esp_hal::Blocking>::new(
        p.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(SPI_MHZ))
            .with_mode(SpiMode::_0),
    )
    .expect("spi")
    .with_sck(p.GPIO14)
    .with_mosi(p.GPIO13)
    .with_miso(p.GPIO12);
    let mut dc = Output::new(p.GPIO2, Level::Low, OutputConfig::default());
    let mut cs = Output::new(p.GPIO15, Level::High, OutputConfig::default());

    // Panel identity probe (runs before the display takes the bus): the CYD
    // family ships ILI9341, ST7789V or ILI9342 depending on revision, and
    // mipidsi init cannot detect the wrong one over write-only SPI.
    let panel_ids = read_panel_ids(&mut spi, &mut dc, &mut cs);
    // XPT2046 (touch, CS=GPIO33) shares the bus — its reply doubles as a
    // SPI+MISO liveness check: the ILI9341 SDO is typically unwired on CYDs,
    // so panel-ID reads read zeros even on a healthy bus.
    let touch = read_xpt2046(&mut spi, &mut xpt_cs);
    xpt_cs.set_high();
    let mut banner = [0u8; 96];
    let banner_len = {
        let mut n = 0;
        banner[..5].copy_from_slice(b"PANEL");
        n += 5;
        for (i, reg) in [0x04u8, 0xD3, 0xBF].into_iter().enumerate() {
            banner[n] = b' ';
            banner[n + 1] = HEX[(reg >> 4) as usize];
            banner[n + 2] = HEX[(reg & 0xF) as usize];
            banner[n + 3] = b'=';
            n += 4;
            for b in panel_ids[i] {
                banner[n] = HEX[(b >> 4) as usize];
                banner[n + 1] = HEX[(b & 0xF) as usize];
                n += 2;
            }
        }
        let touch_txt = b" TOUCH=";
        banner[n..n + 7].copy_from_slice(touch_txt);
        n += 7;
        for v in touch {
            banner[n] = HEX[(v >> 12) as usize];
            banner[n + 1] = HEX[((v >> 8) & 0xF) as usize];
            banner[n + 2] = HEX[((v >> 4) & 0xF) as usize];
            banner[n + 3] = HEX[(v & 0xF) as usize];
            n += 4;
        }
        n
    };
    static BANNER: static_cell::StaticCell<([u8; 96], usize)> =
        static_cell::StaticCell::new();
    let banner = BANNER.init((banner, banner_len));

    // No RST line wired on the CYD (tied to reset rail).
    let delay = Delay::new();
    let bus = ExclusiveDevice::new(spi, cs, delay).expect("spi bus");

    // mipidsi needs a &'static mut command buffer: heap-free StaticCell.
    static DBUF: static_cell::StaticCell<[u8; 512]> = static_cell::StaticCell::new();
    let dbuf = DBUF.init([0u8; 512]);
    let di = SpiInterface::new(bus, dc, dbuf);

    #[cfg(all(feature = "ili9341", not(any(feature = "st7789", feature = "ili9342", feature = "st7796"))))]
    let mut builder = mipidsi::Builder::new(mipidsi::models::ILI9341Rgb565, di);
    #[cfg(all(feature = "st7789", not(any(feature = "ili9341", feature = "ili9342", feature = "st7796"))))]
    let mut builder = mipidsi::Builder::new(mipidsi::models::ST7789, di);
    #[cfg(all(feature = "ili9342", not(any(feature = "ili9341", feature = "st7789", feature = "st7796"))))]
    let mut builder = mipidsi::Builder::new(mipidsi::models::ILI9342CRgb565, di);
    #[cfg(all(feature = "st7796", not(any(feature = "ili9341", feature = "st7789", feature = "ili9342"))))]
    let mut builder = mipidsi::Builder::new(mipidsi::models::ST7796, di);
    builder = builder
        .color_order(ColorOrder::Bgr)
        .display_size(SCREEN_W as u16, SCREEN_H as u16);
    #[cfg(all(feature = "st7796", not(feature = "mirror")))]
    {
        // slint-demo-proven: flipped vertical, inverted colors
        builder = builder
            .orientation(Orientation::new().flip_vertical())
            .invert_colors(mipidsi::options::ColorInversion::Inverted);
    }
    #[cfg(all(feature = "st7796", feature = "mirror"))]
    {
        // mirror-parity variant for scanner experiments (no flips)
        builder = builder
            .orientation(Orientation::new())
            .invert_colors(mipidsi::options::ColorInversion::Inverted);
    }
    #[cfg(all(not(feature = "st7796"), feature = "invert"))]
    {
        builder = builder.invert_colors(mipidsi::options::ColorInversion::Inverted);
    }
    #[cfg(all(not(feature = "st7796"), not(feature = "ili9342")))]
    {
        builder = builder.orientation(Orientation::new());
    }
    let mut display = builder
        .init(&mut Delay::new())
        .expect("display init");

    clear_bg(&mut display, false);

    let mut inverted = false;

    // UART0 = the CH340 USB-serial port the host drives
    let uart = Uart::new(p.UART0, UartConfig::default().with_baudrate(115_200))
        .expect("uart0")
        .with_tx(p.GPIO1)
        .with_rx(p.GPIO3)
        .into_async();
    let (mut rx, mut tx) = uart.split();

    tx.write_all(FW_ID.as_bytes()).await.ok();
    tx.write_all(b" ready ").await.ok();
    tx.write_all(&banner.0[..banner.1]).await.ok();
    tx.write_all(b"\n").await.ok();

    let mut line = [0u8; LINE_MAX];
    let mut len = 0usize;
    let mut overflow = false;
    loop {
        let mut b = [0u8; 1];
        let n = rx.read_async(&mut b).await.unwrap_or(0);
        if n == 0 {
            continue;
        }
        let c = b[0];
        if c == b'\n' || c == b'\r' {
            if overflow {
                reply_bytes(&mut tx, b"ERR TOOLONG\n").await;
            } else if len > 0 {
                handle_line(&line[..len], &mut display, &mut tx, banner, &mut inverted)
                    .await;
            }
            len = 0;
            overflow = false;
            continue;
        }
        if len >= LINE_MAX {
            overflow = true;
        } else {
            line[len] = c;
            len += 1;
        }
    }
}

async fn handle_line(
    line: &[u8],
    display: &mut Display,
    tx: &mut Tx,
    banner: &'static ([u8; 96], usize),
    inverted: &mut bool,
) {
    if line == b"ID" {
        reply_bytes(tx, FW_ID.as_bytes()).await;
        reply_bytes(tx, b"\n").await;
        return;
    }
    if line == b"DIAG" {
        reply_bytes(tx, &banner.0[..banner.1]).await;
        reply_bytes(tx, b"\n").await;
        return;
    }
    if line == b"INV" {
        *inverted = !*inverted;
        clear_bg(display, *inverted);
        if *inverted {
            reply_bytes(tx, b"INVERTED 1\n").await;
        } else {
            reply_bytes(tx, b"INVERTED 0\n").await;
        }
        return;
    }
    if line == b"ECCH" {
        use core::sync::atomic::Ordering;
        let now = !ECC_HIGH.load(Ordering::Relaxed);
        ECC_HIGH.store(now, Ordering::Relaxed);
        if now {
            reply_bytes(tx, b"ECC HIGH\n").await;
        } else {
            reply_bytes(tx, b"ECC MEDIUM\n").await;
        }
        return;
    }
    if line == b"CLR" {
        clear_bg(display, *inverted);
        reply_bytes(tx, b"CLEARED\n").await;
        return;
    }
    // `QR <hex>` renders at max size, centered; `QRS <cap-px> <hex>` caps the
    // QR edge; `QRP <x> <y> <hex>` renders a 160px-capped QR anchored at
    // (x, y) — the host steps this around the screen so the scanner's aim
    // is swept across QR positions without touching the mount.
    let (cap, anchor, hex): (usize, Option<(usize, usize)>, &[u8]) =
        if line.len() >= 3 && &line[..3] == b"QR " {
            (QR_USABLE_PX, None, &line[3..])
        } else if line.len() >= 4 && &line[..4] == b"QRS " {
            let rest = &line[4..];
            let split = match rest.iter().position(|&c| c == b' ') {
                Some(p) => p,
                None => {
                    reply_bytes(tx, b"ERR UNKNOWN\n").await;
                    return;
                }
            };
            let cap = match parse_u16(&rest[..split]) {
                Some(v) if (40..=QR_USABLE_PX as u16).contains(&v) => v as usize,
                _ => {
                    reply_bytes(tx, b"ERR BADCAP\n").await;
                    return;
                }
            };
            (cap, None, &rest[split + 1..])
        } else if line.len() >= 4 && &line[..4] == b"QRP " {
            let rest = &line[4..];
            let mut parts = rest.split(|&c| c == b' ');
            let x = parts.next().and_then(parse_u16);
            let y = parts.next().and_then(parse_u16);
            let hex = parts.next().unwrap_or(&[]);
            match (x, y) {
                (Some(x), Some(y))
                    if (x as usize) < SCREEN_W && (y as usize) < SCREEN_H =>
                {
                    (160, Some((x as usize, y as usize)), hex)
                }
                _ => {
                    reply_bytes(tx, b"ERR BADPOS\n").await;
                    return;
                }
            }
        } else {
            reply_bytes(tx, b"ERR UNKNOWN\n").await;
            return;
        };
    if hex.len() > 2 * PAYLOAD_MAX || hex.len() % 2 != 0 {
        reply_bytes(tx, b"ERR TOOLONG\n").await;
        return;
    }
    let mut payload = [0u8; PAYLOAD_MAX];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < hex.len() {
        let (hi, lo) = match (hex_val(hex[i]), hex_val(hex[i + 1])) {
            (Some(h), Some(l)) => (h, l),
            _ => {
                reply_bytes(tx, b"ERR BADHEX\n").await;
                return;
            }
        };
        payload[n] = (hi << 4) | lo;
        n += 1;
        i += 2;
    }
    let text = match core::str::from_utf8(&payload[..n]) {
        Ok(s) => s,
        Err(_) => {
            reply_bytes(tx, b"ERR NOTUTF8\n").await;
            return;
        }
    };
    match render_qr(display, text, cap, anchor, *inverted) {
        Ok((modules, scale)) => {
            reply_bytes(tx, b"RENDERED ").await;
            reply_u16(tx, modules as u16).await;
            reply_bytes(tx, b" ").await;
            reply_u16(tx, scale as u16).await;
            reply_bytes(tx, b" ").await;
            reply_u16(tx, n as u16).await;
            reply_bytes(tx, b"\n").await;
        }
        Err(_) => reply_bytes(tx, b"ERR QRFAIL\n").await,
    }
}

/// Render `text` as a QR (ECC Medium) centered on a white screen.
/// Returns (module count, pixels per module).
fn render_qr(
    display: &mut Display,
    text: &str,
    cap_px: usize,
    anchor: Option<(usize, usize)>,
    inverted: bool,
) -> Result<(usize, usize), ()> {
    // Inverted mode (film negative): white modules on black — less emission,
    // less sensor bloom; only decodable if the engine supports inverse codes.
    let (bg, fg) = if inverted { (BLACK, WHITE) } else { (WHITE, BLACK) };
    let mut temp = [0u8; QR_BUF];
    let mut out = [0u8; QR_BUF];
    let ecc = if ECC_HIGH.load(core::sync::atomic::Ordering::Relaxed) {
        QrCodeEcc::High
    } else {
        QrCodeEcc::Medium
    };
    let qr = QrCode::encode_text(
        text,
        &mut temp,
        &mut out,
        ecc,
        Version::MIN,
        Version::MAX,
        None,
        true,
    )
    .map_err(|_| ())?;
    let size = qr.size() as usize;
    let total = size + 2 * QUIET_MODULES;
    let scale = (cap_px / total).max(1);
    let px = total * scale;
    let (x0, y0) = match anchor {
        Some((ax, ay)) => (ax.min(SCREEN_W - 1), ay.min(SCREEN_H - 1)),
        None => (
            (SCREEN_W.saturating_sub(px)) / 2,
            (SCREEN_H.saturating_sub(px)) / 2,
        ),
    };

    let mut row;
    for y in 0..SCREEN_H {
        row = [bg; SCREEN_W];
        if y >= y0 && y < y0 + px {
            let my = (y - y0) / scale;
            if my >= QUIET_MODULES && my < QUIET_MODULES + size {
                let m_y = my - QUIET_MODULES;
                for mx in 0..size {
                    if qr.get_module(mx as i32, m_y as i32) {
                        let x_start = x0 + (mx + QUIET_MODULES) * scale;
                        for x in x_start..x_start + scale {
                            if x < SCREEN_W {
                                row[x] = fg;
                            }
                        }
                    }
                }
            }
        }
        // Draw the band, repeated `scale` rows tall, while building it once.
        display
            .set_pixels(0, y as u16, SCREEN_W as u16 - 1, y as u16, row)
            .map_err(|_| ())?;
    }
    Ok((size, scale))
}

fn clear_bg(display: &mut Display, inverted: bool) {
    let row = [if inverted { BLACK } else { WHITE }; SCREEN_W];
    for y in 0..SCREEN_H {
        display
            .set_pixels(0, y as u16, SCREEN_W as u16 - 1, y as u16, row)
            .ok();
    }
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn parse_u16(s: &[u8]) -> Option<u16> {
    if s.is_empty() || s.len() > 5 {
        return None;
    }
    let mut v: u16 = 0;
    for &c in s {
        let d = match hex_val(c) {
            Some(d) if d < 10 => d,
            _ => return None,
        };
        v = v.checked_mul(10)?.checked_add(d as u16)?;
    }
    Some(v)
}

async fn reply_bytes(tx: &mut Tx, s: &[u8]) {
    tx.write_all(s).await.ok();
}

async fn reply_u16(tx: &mut Tx, v: u16) {
    let mut buf = [0u8; 6];
    let mut i = buf.len();
    let mut val = v;
    loop {
        i -= 1;
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
        if val == 0 {
            break;
        }
    }
    tx.write_all(&buf[i..]).await.ok();
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
