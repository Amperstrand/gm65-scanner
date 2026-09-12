//! 480x800 LCD for the playground, rendered with the same embedded-graphics
//! stack the firmware uses and mirroring the example firmware's screens
//! (examples/stm32f469i-disco/src/display.rs theme + layout constants) so
//! the browser UX matches the device. Payload classification and text
//! wrapping come from the crate's own `classify_payload` / `word_wrap` —
//! the same code paths the firmware runs.

use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use gm65_scanner::display_util::word_wrap;
use gm65_scanner::{classify_payload, PayloadType};
use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};
use wasm_bindgen::JsCast;

pub const W: u32 = 480;
pub const H: u32 = 800;

mod theme {
    use embedded_graphics::pixelcolor::Rgb888;
    pub const BG_DARK: Rgb888 = Rgb888::new(0x18, 0x18, 0x18);
    pub const ACCENT_CYAN: Rgb888 = Rgb888::new(0x00, 0xFF, 0xFF);
    pub const SUCCESS: Rgb888 = Rgb888::new(0x00, 0xFF, 0x00);
    pub const ERROR: Rgb888 = Rgb888::new(0xFF, 0x00, 0x00);
    pub const TEXT_PRIMARY: Rgb888 = Rgb888::new(0xFF, 0xFF, 0xFF);
    pub const TEXT_SECONDARY: Rgb888 = Rgb888::new(0xA0, 0xA0, 0xA0);
}

const X_LABEL: i32 = 20;
const Y_HOME_TITLE: i32 = 80;
const Y_HOME_READY: i32 = 120;
const Y_HOME_SCANNER_ROW: i32 = 200;
const Y_HOME_HELP: i32 = 500;
const Y_RESULT_TYPE_NAME: i32 = 60;
const Y_RESULT_START: i32 = 100;
const WRAP_CHARS: usize = 44;

pub struct Lcd {
    pixels: Vec<u8>,
}

impl Lcd {
    pub fn new() -> Self {
        let mut lcd = Self {
            pixels: vec![0; (W * H * 4) as usize],
        };
        lcd.fill(theme::BG_DARK);
        lcd
    }

    fn fill(&mut self, color: Rgb888) {
        let (chunks, _) = self.pixels.as_chunks_mut::<4>();
        for px in chunks {
            px[0] = color.r();
            px[1] = color.g();
            px[2] = color.b();
            px[3] = 0xFF;
        }
    }

    fn text(&mut self, s: &str, x: i32, y: i32, color: Rgb888) {
        let style = MonoTextStyle::new(&FONT_10X20, color);
        // Draw into a scratch that aliases the RGBA buffer via DrawTarget
        // on Lcd itself.
        let _ = Text::new(s, Point::new(x, y), style).draw(self);
    }

    pub fn paint(&self) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Some(doc) = window.document() else { return };
        let Some(el) = doc.get_element_by_id("lcd") else {
            return;
        };
        let Ok(canvas) = el.dyn_into::<web_sys::HtmlCanvasElement>() else {
            return;
        };
        let Ok(ctx_obj) = canvas.get_context("2d") else {
            return;
        };
        let Some(ctx_any) = ctx_obj else { return };
        let Ok(ctx) = ctx_any.dyn_into::<web_sys::CanvasRenderingContext2d>() else {
            return;
        };
        if let Ok(img) = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            wasm_bindgen::Clamped(&self.pixels),
            W,
            H,
        ) {
            let _ = ctx.put_image_data(&img, 0.0, 0.0);
        }
    }

    pub fn boot(&mut self) {
        self.fill(theme::BG_DARK);
        self.text("gm65-scanner", X_LABEL, Y_HOME_TITLE, theme::ACCENT_CYAN);
        self.text(
            "wasm playground",
            X_LABEL,
            Y_HOME_READY,
            theme::TEXT_SECONDARY,
        );
        self.text(
            "press INIT on the side",
            X_LABEL,
            Y_HOME_HELP,
            theme::TEXT_SECONDARY,
        );
        self.paint();
    }

    pub fn home(&mut self, model: &str) {
        self.fill(theme::BG_DARK);
        self.text("GM65 HOST", X_LABEL, Y_HOME_TITLE, theme::ACCENT_CYAN);
        self.text("READY", X_LABEL, Y_HOME_READY, theme::SUCCESS);
        self.text("Scanner:", X_LABEL, Y_HOME_SCANNER_ROW, theme::TEXT_PRIMARY);
        self.text(model, 140, Y_HOME_SCANNER_ROW, theme::TEXT_PRIMARY);
        self.text(
            "START SCANNING",
            X_LABEL,
            Y_HOME_HELP,
            theme::TEXT_SECONDARY,
        );
        self.paint();
    }

    pub fn scanning(&mut self, policy: &str) {
        self.fill(theme::BG_DARK);
        self.text("SCANNING", X_LABEL, Y_HOME_TITLE, theme::ACCENT_CYAN);
        self.text(
            "aim at a QR code",
            X_LABEL,
            Y_HOME_READY,
            theme::TEXT_SECONDARY,
        );
        self.text("policy:", X_LABEL, Y_HOME_SCANNER_ROW, theme::TEXT_PRIMARY);
        self.text(policy, 140, Y_HOME_SCANNER_ROW, theme::TEXT_PRIMARY);
        self.text(
            "waiting for decode...",
            X_LABEL,
            Y_HOME_HELP,
            theme::TEXT_SECONDARY,
        );
        self.paint();
    }

    pub fn result(&mut self, payload: &str) {
        let ptype = classify_payload(payload.as_bytes());
        self.fill(theme::BG_DARK);
        self.text(
            type_name(&ptype),
            X_LABEL,
            Y_RESULT_TYPE_NAME,
            theme::ACCENT_CYAN,
        );
        let mut y = Y_RESULT_START;
        for line in word_wrap(payload, WRAP_CHARS).iter().take(10) {
            self.text(line, X_LABEL, y, theme::TEXT_PRIMARY);
            y += 22;
        }
        let len_line = format!("{} bytes", payload.len());
        self.text(&len_line, X_LABEL, y + 8, theme::TEXT_SECONDARY);
        self.qr_mirror(payload, 620);
        self.paint();
    }

    /// Mirror the decoded payload as a QR (same encoder + ECC as the
    /// firmware example's qr_display.rs) in the lower band of the screen.
    fn qr_mirror(&mut self, payload: &str, top: i32) {
        const BUF: usize = 4096;
        let mut temp = [0u8; BUF];
        let mut out = [0u8; BUF];
        let Ok(qr) = QrCode::encode_text(
            payload,
            &mut temp,
            &mut out,
            QrCodeEcc::Medium,
            Version::MIN,
            Version::MAX,
            None,
            true,
        ) else {
            self.text("(payload too large for QR)", X_LABEL, top, theme::ERROR);
            return;
        };
        let border = 2;
        let total = qr.size() + border * 2;
        let scale = ((W - 40) / total as u32)
            .min(160 / total.max(1) as u32)
            .max(1);
        let scaled = total as u32 * scale;
        let x0 = ((W - scaled) / 2) as i32;
        let y0 = top;
        // White module field, then dark modules on top.
        let _ = embedded_graphics::primitives::Rectangle::new(
            Point::new(x0, y0),
            Size::new(scaled, scaled),
        )
        .into_styled(embedded_graphics::primitives::PrimitiveStyle::with_fill(
            Rgb888::WHITE,
        ))
        .draw(self);
        for y in 0..qr.size() {
            for x in 0..qr.size() {
                if qr.get_module(x, y) {
                    let _ = embedded_graphics::primitives::Rectangle::new(
                        Point::new(
                            x0 + (border + x) * scale as i32,
                            y0 + (border + y) * scale as i32,
                        ),
                        Size::new(scale, scale),
                    )
                    .into_styled(embedded_graphics::primitives::PrimitiveStyle::with_fill(
                        Rgb888::BLACK,
                    ))
                    .draw(self);
                }
            }
        }
    }
}

fn type_name(pt: &PayloadType) -> &'static str {
    match pt {
        PayloadType::CashuV4 => "Cashu V4 Token",
        PayloadType::CashuV3 => "Cashu V3 Token",
        PayloadType::UrFragment => "UR Fragment",
        PayloadType::Url => "URL",
        PayloadType::PlainText => "Plain Text",
        PayloadType::Binary => "Binary Data",
    }
}

impl OriginDimensions for Lcd {
    fn size(&self) -> Size {
        Size::new(W, H)
    }
}

impl DrawTarget for Lcd {
    type Color = Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(Point { x, y }, color) in pixels {
            if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                continue;
            }
            let idx = ((y as u32 * W + x as u32) * 4) as usize;
            self.pixels[idx] = color.r();
            self.pixels[idx + 1] = color.g();
            self.pixels[idx + 2] = color.b();
            self.pixels[idx + 3] = 0xFF;
        }
        Ok(())
    }
}
