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
use embedded_graphics::primitives::PrimitiveStyle;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::Text;
use gm65_scanner::display_util::word_wrap;
use gm65_scanner::settings::{AimSetting, LightSetting, ReadMode, ScannerSettings};
use gm65_scanner::{classify_payload, PayloadType};
use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};
use wasm_bindgen::JsCast;

use crate::ui::{rows_for, RowId, Screen, UiState};

pub const W: u32 = 480;
pub const H: u32 = 800;

pub const ROW_H: i32 = 64;
pub const ROW_X: i32 = 24;
pub const ROW_W: i32 = 480 - 48;

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

    /// Render the current screen. Row placement mirrors `ui::rows_for`
    /// (same constants) so hit-testing and drawing cannot drift apart.
    pub fn render(&mut self, state: &UiState, settings: &ScannerSettings, module_bits: u8) {
        self.fill(theme::BG_DARK);
        match state.screen {
            Screen::Boot => {
                self.text("gm65-scanner", X_LABEL, 80, theme::ACCENT_CYAN);
                self.text("wasm playground", X_LABEL, 120, theme::TEXT_SECONDARY);
                if let Some(err) = &state.last_error {
                    self.text(&format!("init ✗ {err}"), X_LABEL, 200, theme::ERROR);
                } else {
                    self.text(
                        "press INIT on the side",
                        X_LABEL,
                        500,
                        theme::TEXT_SECONDARY,
                    );
                }
            }
            Screen::Home => {
                self.text("GM65 HOST", X_LABEL, 60, theme::ACCENT_CYAN);
                if state.last_error.is_some() {
                    self.text("INIT FAILED", X_LABEL, 100, theme::ERROR);
                } else {
                    self.text("READY", X_LABEL, 100, theme::SUCCESS);
                }
                self.text(
                    &format!("scanner: {}", state.model),
                    X_LABEL,
                    120,
                    theme::TEXT_SECONDARY,
                );
                for (id, y) in rows_for(Screen::Home) {
                    match id {
                        RowId::Start => self.row(*y, "Start scan", "▶", true),
                        RowId::SettingsNav => self.row(*y, "Settings", "dry ⚙", false),
                        _ => {}
                    }
                }
                self.text(
                    "tap rows · host tools in the wing",
                    X_LABEL,
                    740,
                    theme::TEXT_SECONDARY,
                );
            }
            Screen::Scanning => {
                self.text("SCANNING", X_LABEL, 60, theme::ACCENT_CYAN);
                self.text("aim at a QR code", X_LABEL, 100, theme::TEXT_SECONDARY);
                self.text(
                    &format!("policy: {}", state.policy_name),
                    X_LABEL,
                    120,
                    theme::TEXT_SECONDARY,
                );
                for (id, y) in rows_for(Screen::Scanning) {
                    if *id == RowId::Stop {
                        self.row(*y, "Stop", "■", true);
                    }
                }
            }
            Screen::Result => {
                let payload = state.payload.as_deref().unwrap_or("");
                let ptype = classify_payload(payload.as_bytes());
                self.text(type_name(&ptype), X_LABEL, 50, theme::ACCENT_CYAN);
                let mut y = 90;
                for line in word_wrap(payload, WRAP_CHARS).iter().take(8) {
                    self.text(line, X_LABEL, y, theme::TEXT_PRIMARY);
                    y += 22;
                }
                self.text(
                    &format!("{} bytes", payload.len()),
                    X_LABEL,
                    y + 6,
                    theme::TEXT_SECONDARY,
                );
                self.qr_mirror(payload, 530, 180);
                for (id, y) in rows_for(Screen::Result) {
                    if *id == RowId::Back {
                        self.row(*y, "Back", "←", false);
                    }
                }
            }
            Screen::Settings => {
                self.text("SETTINGS (DRY)", X_LABEL, 60, theme::ACCENT_CYAN);
                self.text(
                    "writes go to the virtual module only",
                    X_LABEL,
                    90,
                    theme::TEXT_SECONDARY,
                );
                let aim = match settings.aim {
                    AimSetting::Off => "off",
                    AimSetting::Reading => "reading",
                    AimSetting::Always => "always",
                };
                let light = match settings.light {
                    LightSetting::Off => "off",
                    LightSetting::Reading => "reading",
                    LightSetting::Always => "always",
                };
                let mode = match settings.read_mode {
                    ReadMode::Manual => "manual",
                    ReadMode::Command => "command",
                    ReadMode::Continuous => "continuous",
                    ReadMode::Induction => "induction",
                };
                for (id, y) in rows_for(Screen::Settings) {
                    match id {
                        RowId::Buzzer => {
                            self.row(*y, "Buzzer", on_off(settings.buzzer), false);
                        }
                        RowId::Aim => self.row(*y, "Aim LED", aim, false),
                        RowId::Light => self.row(*y, "Light", light, false),
                        RowId::Mode => self.row(*y, "Read mode", mode, false),
                        RowId::Back => self.row(*y, "Back", "←", false),
                        _ => {}
                    }
                }
                self.text(
                    &format!("SETTINGS = 0x{module_bits:02X}"),
                    X_LABEL,
                    760,
                    theme::TEXT_SECONDARY,
                );
            }
        }
        self.paint();
    }

    /// A tappable row: bordered box, label left, value right.
    fn row(&mut self, y: i32, label: &str, value: &str, accent: bool) {
        let border = if accent {
            theme::ACCENT_CYAN
        } else {
            theme::TEXT_SECONDARY
        };
        let _ = Rectangle::new(Point::new(ROW_X, y), Size::new(ROW_W as u32, ROW_H as u32))
            .into_styled(PrimitiveStyle::with_fill(Rgb888::new(0x10, 0x18, 0x28)))
            .draw(self);
        let _ = Rectangle::new(Point::new(ROW_X, y), Size::new(ROW_W as u32, ROW_H as u32))
            .into_styled(PrimitiveStyle::with_stroke(border, 2))
            .draw(self);
        let text_y = y + ROW_H / 2 + 7;
        self.text(label, ROW_X + 16, text_y, theme::TEXT_PRIMARY);
        let value_x = ROW_X + ROW_W - 16 - value.chars().count() as i32 * 10;
        self.text(value, value_x, text_y, theme::ACCENT_CYAN);
    }

    /// Mirror the decoded payload as a QR (same encoder + ECC as the
    /// firmware example's qr_display.rs), sized to fit `max_h` pixels.
    fn qr_mirror(&mut self, payload: &str, top: i32, max_h: u32) {
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
            .min(max_h / total.max(1) as u32)
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

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
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
