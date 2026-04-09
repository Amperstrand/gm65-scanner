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

pub fn render_qr_code(fb: &mut impl DrawTarget<Color = Rgb888>, text: &str) -> bool {
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

    let fb_width: i32 = 480;
    let fb_height: i32 = 800;
    let border = 2;
    let qr_size = qr.size();
    let total = qr_size + border * 2;

    let max_scale_x = (fb_width - 40) / total;
    let max_scale_y = (fb_height - 80) / total;
    let scale = max_scale_x.min(max_scale_y).max(1) as u32;

    let qr_pixel_w = total as u32 * scale;
    let qr_pixel_h = total as u32 * scale;
    let offset_x = (fb_width as u32 - qr_pixel_w) / 2;
    let offset_y = 20 + (fb_height as u32 - qr_pixel_h - 40) / 2;

    let _ = fb.clear(Rgb888::BLACK);

    for qr_y in 0..qr_size {
        for qr_x in 0..qr_size {
            let dark = qr.get_module(qr_x, qr_y);
            let color = if dark { BLACK } else { WHITE };

            let px = offset_x + (qr_x + border) as u32 * scale;
            let py = offset_y + (qr_y + border) as u32 * scale;

            if px + scale <= fb_width as u32 && py + scale <= fb_height as u32 {
                let _ = fb.fill_solid(
                    &Rectangle::new(Point::new(px as i32, py as i32), Size::new(scale, scale)),
                    color,
                );
            }
        }
    }

    let style = MonoTextStyle::new(&FONT_10X20, Rgb888::new(0, 255, 255));
    let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
    let label = truncate_str(text, 50);
    Text::with_text_style(
        label,
        Point::new(fb_width / 2, (offset_y + qr_pixel_h + 10) as i32),
        style,
        center,
    )
    .draw(fb)
    .ok();

    true
}

pub fn render_qr_mirror(fb: &mut impl DrawTarget<Color = Rgb888>, data: &[u8]) {
    match core::str::from_utf8(data) {
        Ok(text) if data.len() <= 200 => {
            if !render_qr_code(fb, text) {
                let _ = fb.clear(Rgb888::BLACK);
                let style = MonoTextStyle::new(&FONT_10X20, Rgb888::new(255, 0, 0));
                let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
                Text::with_text_style("QR encode failed", Point::new(240, 240), style, center)
                    .draw(fb)
                    .ok();
            }
        }
        _ => {
            let _ = fb.clear(Rgb888::BLACK);
            let style = MonoTextStyle::new(&FONT_10X20, Rgb888::new(255, 0, 0));
            let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
            Text::with_text_style("Data too long for QR", Point::new(240, 240), style, center)
                .draw(fb)
                .ok();
        }
    }
}
