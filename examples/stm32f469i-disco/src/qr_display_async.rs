use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::Rectangle,
    text::{Alignment, Text, TextStyleBuilder},
};
use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};

use crate::display_embassy::{FramebufferView, FB_HEIGHT, FB_WIDTH};

const BLACK: Rgb565 = Rgb565::BLACK;
const WHITE: Rgb565 = Rgb565::WHITE;
const QR_BUF_SIZE: usize = Version::MAX.buffer_len();

pub fn render_qr_code(fb: &mut FramebufferView<'_>, text: &str) -> bool {
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

    let fb_width: i32 = FB_WIDTH as i32;
    let fb_height: i32 = FB_HEIGHT as i32;
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

    fb.clear(Rgb565::BLACK);

    for qr_y in 0..qr_size {
        for qr_x in 0..qr_size {
            let dark = qr.get_module(qr_x, qr_y);
            let color = if dark { BLACK } else { WHITE };

            let px = offset_x + (qr_x + border) as u32 * scale;
            let py = offset_y + (qr_y + border) as u32 * scale;

            if px + scale <= fb_width as u32 && py + scale <= fb_height as u32 {
                let _ = fb.fill_contiguous(
                    &Rectangle::new(Point::new(px as i32, py as i32), Size::new(scale, scale)),
                    core::iter::once(color),
                );
            }
        }
    }

    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
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

pub fn render_qr_mirror(fb: &mut FramebufferView<'_>, data: &[u8]) {
    match core::str::from_utf8(data) {
        Ok(text) if data.len() <= 200 => {
            if !render_qr_code(fb, text) {
                render_status(fb, "QR encode failed");
            }
        }
        _ => {
            render_status(fb, "Data too long for QR");
        }
    }
}

pub fn render_status(fb: &mut FramebufferView<'_>, message: &str) {
    fb.clear(Rgb565::BLACK);
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(
        truncate_str(message, 60),
        Point::new(240, 240),
        style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_scan_result(fb: &mut FramebufferView<'_>, data: &[u8]) {
    fb.clear(Rgb565::BLACK);

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let label_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_YELLOW);
    let value_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style("Scan Result", Point::new(240, 30), title_style, center_text)
        .draw(fb)
        .ok();

    let mut y = 60u32;

    Text::new("Size:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    let len_str = format_len(data.len());
    Text::new(&len_str, Point::new(120, y as i32), value_style)
        .draw(fb)
        .ok();
    y += 30;

    Text::new("Data:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    y += 25;

    let data_str = core::str::from_utf8(data).unwrap_or("<binary data>");
    let chars_per_line = 76;
    let mut offset = 0;
    while offset < data_str.len() && y < FB_HEIGHT as u32 - 20 {
        let end = core::cmp::min(offset + chars_per_line, data_str.len());
        let line = &data_str[offset..end];
        Text::new(line, Point::new(20, y as i32), value_style)
            .draw(fb)
            .ok();
        offset = end;
        y += 22;
    }
}

fn format_len(len: usize) -> heapless::String<16> {
    let mut s = heapless::String::new();
    let mut n = len;
    let mut digits = [0u8; 8];
    let mut i = 0;
    if n == 0 {
        digits[0] = 0;
        i = 1;
    } else {
        while n > 0 && i < 8 {
            digits[i] = (n % 10) as u8;
            n /= 10;
            i += 1;
        }
    }
    for j in (0..i).rev() {
        let _ = s.push((b'0' + digits[j]) as char);
    }
    let _ = s.push_str(" bytes");
    s
}

fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}
