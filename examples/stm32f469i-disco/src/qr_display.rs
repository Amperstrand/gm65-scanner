use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::Rectangle,
    text::{Alignment, Text, TextStyleBuilder},
};
use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};

use crate::display_utils::truncate_str;

const BLACK: Rgb888 = Rgb888::BLACK;
const WHITE: Rgb888 = Rgb888::WHITE;
const QR_BUF_SIZE: usize = Version::MAX.buffer_len();
const FB_WIDTH: i32 = 480;
const FB_HEIGHT: i32 = 800;
const QR_BORDER_MODULES: i32 = 2;
const QR_MARGIN_X: i32 = 40;
const QR_MARGIN_Y: i32 = 80;
const QR_TOP_OFFSET: i32 = 20;
const MIN_SCALE: i32 = 1;
const QR_LABEL_MAX_LEN: usize = 50;
const QR_LABEL_BOTTOM_OFFSET: i32 = 10;
pub const QR_MAX_DATA_LEN: usize = 200;
const QR_ERROR_CENTER: i32 = 240;
const YIELD_INTERVAL: u32 = 8;

#[cfg(not(feature = "scanner-async"))]
pub fn render_qr_code(fb: &mut impl DrawTarget<Color = Rgb888>, text: &str) -> bool {
    render_qr_code_with_yield(fb, text, || {})
}

pub fn render_qr_code_with_yield(
    fb: &mut impl DrawTarget<Color = Rgb888>,
    text: &str,
    mut yield_fn: impl FnMut(),
) -> bool {
    let mut temp_buf = [0u8; QR_BUF_SIZE];
    let mut out_buf = [0u8; QR_BUF_SIZE];

    let qr = match QrCode::encode_text(
        text,
        &mut temp_buf,
        &mut out_buf,
        QrCodeEcc::Medium,
        Version::MIN,
        Version::MAX,
        None,
        true,
    ) {
        Ok(qr) => qr,
        Err(_) => return false,
    };

    let qr_size = qr.size();
    let total = qr_size + QR_BORDER_MODULES * 2;

    let max_total = ((FB_WIDTH - QR_MARGIN_X) / (QR_BORDER_MODULES * 2 + 1))
        .min((FB_HEIGHT - QR_MARGIN_Y) / (QR_BORDER_MODULES * 2 + 1));
    if total > max_total {
        return false;
    }

    let max_scale_x = (FB_WIDTH - QR_MARGIN_X) / total;
    let max_scale_y = (FB_HEIGHT - QR_MARGIN_Y) / total;
    let scale = max_scale_x.min(max_scale_y).max(MIN_SCALE) as u32;

    let qr_pixel_w = total as u32 * scale;
    let qr_pixel_h = total as u32 * scale;
    let offset_x = (FB_WIDTH as u32 - qr_pixel_w) / 2;
    let offset_y = QR_TOP_OFFSET as u32 + (FB_HEIGHT as u32 - qr_pixel_h - QR_MARGIN_X as u32) / 2;

    let _ = fb.clear(Rgb888::BLACK);
    let mut module_count = 0u32;

    for qr_y in 0..qr_size {
        for qr_x in 0..qr_size {
            let dark = qr.get_module(qr_x, qr_y);
            let color = if dark { BLACK } else { WHITE };

            let px = offset_x + (qr_x + QR_BORDER_MODULES) as u32 * scale;
            let py = offset_y + (qr_y + QR_BORDER_MODULES) as u32 * scale;

            if px + scale <= FB_WIDTH as u32 && py + scale <= FB_HEIGHT as u32 {
                let _ = fb.fill_solid(
                    &Rectangle::new(Point::new(px as i32, py as i32), Size::new(scale, scale)),
                    color,
                );
            }

            module_count += 1;
            if module_count.is_multiple_of(YIELD_INTERVAL) {
                yield_fn();
            }
        }
    }

    let style = MonoTextStyle::new(&FONT_10X20, Rgb888::new(0, 255, 255));
    let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
    let label = truncate_str(text, QR_LABEL_MAX_LEN);
    Text::with_text_style(
        label,
        Point::new(
            FB_WIDTH / 2,
            (offset_y + qr_pixel_h + QR_LABEL_BOTTOM_OFFSET as u32) as i32,
        ),
        style,
        center,
    )
    .draw(fb)
    .ok();

    true
}

#[cfg(not(feature = "scanner-async"))]
pub fn render_qr_mirror(fb: &mut impl DrawTarget<Color = Rgb888>, data: &[u8]) {
    render_qr_mirror_with_yield(fb, data, || {});
}

pub fn render_qr_mirror_with_yield(
    fb: &mut impl DrawTarget<Color = Rgb888>,
    data: &[u8],
    mut yield_fn: impl FnMut(),
) {
    match core::str::from_utf8(data) {
        Ok(text) if data.len() <= QR_MAX_DATA_LEN => {
            if !render_qr_code_with_yield(fb, text, &mut yield_fn) {
                let _ = fb.clear(Rgb888::BLACK);
                let style = MonoTextStyle::new(&FONT_10X20, Rgb888::new(255, 0, 0));
                let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
                Text::with_text_style(
                    "QR encode failed",
                    Point::new(QR_ERROR_CENTER, QR_ERROR_CENTER),
                    style,
                    center,
                )
                .draw(fb)
                .ok();
            }
        }
        _ => {
            let _ = fb.clear(Rgb888::BLACK);
            let style = MonoTextStyle::new(&FONT_10X20, Rgb888::new(255, 0, 0));
            let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
            Text::with_text_style(
                "Data too long for QR",
                Point::new(QR_ERROR_CENTER, QR_ERROR_CENTER),
                style,
                center,
            )
            .draw(fb)
            .ok();
        }
    }
}
