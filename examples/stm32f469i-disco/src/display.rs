use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    text::{Alignment, Text, TextStyleBuilder},
};
use stm32f469i_disc::hal::ltdc::LtdcFramebuffer;

use gm65_scanner::{DecodedPayload, PayloadType};

pub const HEIGHT: u32 = 480;

pub fn render_status(fb: &mut LtdcFramebuffer<u16>, message: &str) {
    fb.clear(Rgb565::BLACK).ok();
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(
        truncate_str(message, 60),
        Point::new(400, 240),
        style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_home(fb: &mut LtdcFramebuffer<u16>, scanner_connected: bool, model: &str) {
    fb.clear(Rgb565::BLACK).ok();

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let ok_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_GREEN);
    let err_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_RED);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style("QR Scanner", Point::new(400, 80), title_style, center_text)
        .draw(fb)
        .ok();

    Text::with_text_style("Ready", Point::new(400, 120), style, center_text)
        .draw(fb)
        .ok();

    Text::new("Scanner:", Point::new(20, 200), style)
        .draw(fb)
        .ok();

    if scanner_connected {
        Text::new(model, Point::new(140, 200), ok_style)
            .draw(fb)
            .ok();
    } else {
        Text::new("NOT FOUND", Point::new(140, 200), err_style)
            .draw(fb)
            .ok();
    }

    Text::with_text_style(
        "Scan a QR code or send USB command...",
        Point::new(400, 350),
        style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_error(fb: &mut LtdcFramebuffer<u16>, message: &str) {
    fb.clear(Rgb565::BLACK).ok();
    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::RED);
    let msg_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style("ERROR", Point::new(400, 200), title_style, center_text)
        .draw(fb)
        .ok();
    Text::with_text_style(
        truncate_str(message, 60),
        Point::new(400, 240),
        msg_style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_scan_result(fb: &mut LtdcFramebuffer<u16>, data: &[u8]) {
    let payload = gm65_scanner::decode_payload(data);
    render_decoded_scan(fb, &payload);
}

pub fn render_decoded_scan(fb: &mut LtdcFramebuffer<u16>, payload: &DecodedPayload) {
    fb.clear(Rgb565::BLACK).ok();

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let label_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_YELLOW);
    let value_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let ok_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_GREEN);
    let dim_style = MonoTextStyle::new(&FONT_10X20, Rgb565::new(0x40, 0x40, 0x40));
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style("Scan Result", Point::new(400, 30), title_style, center_text)
        .draw(fb)
        .ok();

    let type_name = type_name(&payload.payload_type);
    Text::with_text_style(type_name, Point::new(400, 60), ok_style, center_text)
        .draw(fb)
        .ok();

    let raw = &payload.raw;
    let len_label = format_u32_len(raw.len());

    let mut y = 100u32;

    Text::new("Size:", Point::new(20, y as i32), label_style)
        .draw(fb)
        .ok();
    Text::new(&len_label, Point::new(120, y as i32), value_style)
        .draw(fb)
        .ok();
    y += 30;

    match payload.payload_type {
        PayloadType::CashuV4 => {
            Text::new("Type:", Point::new(20, y as i32), label_style)
                .draw(fb)
                .ok();
            Text::new("Cashu V4 Token", Point::new(120, y as i32), value_style)
                .draw(fb)
                .ok();
            y += 30;
            render_raw_data(fb, raw, y, &label_style, &value_style, &dim_style);
        }
        PayloadType::CashuV3 => {
            Text::new("Type:", Point::new(20, y as i32), label_style)
                .draw(fb)
                .ok();
            Text::new("Cashu V3 (legacy)", Point::new(120, y as i32), value_style)
                .draw(fb)
                .ok();
            y += 30;
            render_raw_data(fb, raw, y, &label_style, &value_style, &dim_style);
        }
        PayloadType::UrFragment => {
            if let Some(parsed) = gm65_scanner::parse_ur_fragment(raw) {
                let mut frag_str = heapless::String::<32>::new();
                let _ = frag_str.push_str(&format_u32_len(parsed.index as usize));
                let _ = frag_str.push('/');
                let _ = frag_str.push_str(&format_u32_len(parsed.total as usize));
                Text::new("Progress:", Point::new(20, y as i32), label_style)
                    .draw(fb)
                    .ok();
                Text::new(&frag_str, Point::new(160, y as i32), value_style)
                    .draw(fb)
                    .ok();
                y += 30;

                let mut type_str = heapless::String::<32>::new();
                let _ = type_str.push_str("UR Type: ");
                let _ = type_str.push_str(&parsed.ur_type);
                Text::new(&type_str, Point::new(20, y as i32), label_style)
                    .draw(fb)
                    .ok();
                y += 30;
            }
            render_raw_data(fb, raw, y, &label_style, &value_style, &dim_style);
        }
        PayloadType::Url | PayloadType::PlainText | PayloadType::Binary => {
            render_raw_data(fb, raw, y, &label_style, &value_style, &dim_style);
        }
    }
}

fn render_raw_data(
    fb: &mut LtdcFramebuffer<u16>,
    raw: &[u8],
    start_y: u32,
    label_style: &MonoTextStyle<'_, Rgb565>,
    value_style: &MonoTextStyle<'_, Rgb565>,
    _dim_style: &MonoTextStyle<'_, Rgb565>,
) {
    let mut y = start_y;

    Text::new("Data:", Point::new(20, y as i32), *label_style)
        .draw(fb)
        .ok();
    y += 25;

    let data_str = core::str::from_utf8(raw).unwrap_or("<binary data>");
    let chars_per_line = 76;
    let mut offset = 0;
    while offset < data_str.len() && y < HEIGHT - 20 {
        let end = core::cmp::min(offset + chars_per_line, data_str.len());
        let line = &data_str[offset..end];
        Text::new(line, Point::new(20, y as i32), *value_style)
            .draw(fb)
            .ok();
        offset = end;
        y += 22;
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

fn format_u32_len(len: usize) -> heapless::String<16> {
    let mut s = heapless::String::new();
    if len < 10 {
        let _ = s.push((b'0' + len as u8) as char);
    } else if len < 100 {
        let _ = s.push((b'0' + (len / 10) as u8) as char);
        let _ = s.push((b'0' + (len % 10) as u8) as char);
    } else if len < 1000 {
        let _ = s.push((b'0' + (len / 100) as u8) as char);
        let _ = s.push((b'0' + ((len / 10) % 10) as u8) as char);
        let _ = s.push((b'0' + (len % 10) as u8) as char);
    } else {
        let mut n = len;
        let mut digits = [0u8; 8];
        let mut i = 0;
        while n > 0 && i < 8 {
            digits[i] = (n % 10) as u8;
            n /= 10;
            i += 1;
        }
        for j in (0..i).rev() {
            let _ = s.push(digits[j] as char);
        }
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
