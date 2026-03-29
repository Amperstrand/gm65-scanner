use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    text::{Alignment, Text, TextStyleBuilder},
};

use gm65_scanner::{DecodedPayload, PayloadType, ScannerSettings};

use crate::display_utils::{format_byte, format_u32_len, truncate_str};

pub fn render_status(fb: &mut impl DrawTarget<Color = Rgb565>, message: &str) {
    let _ = fb.clear(Rgb565::BLACK);
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(
        truncate_str(message, 60),
        Point::new(DISPLAY_CENTER_X, 240),
        style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_home(fb: &mut impl DrawTarget<Color = Rgb565>, scanner_connected: bool, model: &str) {
    let _ = fb.clear(Rgb565::BLACK);

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let ok_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_GREEN);
    let err_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_RED);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style(
        "QR Scanner",
        Point::new(DISPLAY_CENTER_X, 80),
        title_style,
        center_text,
    )
    .draw(fb)
    .ok();

    Text::with_text_style(
        "Ready",
        Point::new(DISPLAY_CENTER_X, 120),
        style,
        center_text,
    )
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
        Point::new(DISPLAY_CENTER_X, 350),
        style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_error(fb: &mut impl DrawTarget<Color = Rgb565>, message: &str) {
    let _ = fb.clear(Rgb565::BLACK);
    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::RED);
    let msg_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(
        "ERROR",
        Point::new(DISPLAY_CENTER_X, 200),
        title_style,
        center_text,
    )
    .draw(fb)
    .ok();
    Text::with_text_style(
        truncate_str(message, 60),
        Point::new(DISPLAY_CENTER_X, 240),
        msg_style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_scanner_settings(
    fb: &mut impl DrawTarget<Color = Rgb565>,
    settings: ScannerSettings,
) {
    let _ = fb.clear(Rgb565::BLACK);

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let label_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_YELLOW);
    let on_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_GREEN);
    let off_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_RED);
    let val_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);

    Text::with_text_style(
        "Scanner Settings",
        Point::new(DISPLAY_CENTER_X, 30),
        title_style,
        TextStyleBuilder::new().alignment(Alignment::Center).build(),
    )
    .draw(fb)
    .ok();

    let mut y = 80i32;
    let x_label = 20;
    let x_value = 200;

    draw_toggle(
        fb,
        x_label,
        x_value,
        y,
        "Sound",
        settings.contains(ScannerSettings::SOUND),
        &label_style,
        &on_style,
        &off_style,
    );
    y += 35;
    draw_toggle(
        fb,
        x_label,
        x_value,
        y,
        "Aim/Laser",
        settings.contains(ScannerSettings::AIM),
        &label_style,
        &on_style,
        &off_style,
    );
    y += 35;
    draw_toggle(
        fb,
        x_label,
        x_value,
        y,
        "Light",
        settings.contains(ScannerSettings::LIGHT),
        &label_style,
        &on_style,
        &off_style,
    );
    y += 35;
    draw_toggle(
        fb,
        x_label,
        x_value,
        y,
        "Continuous",
        settings.contains(ScannerSettings::CONTINUOUS),
        &label_style,
        &on_style,
        &off_style,
    );
    y += 35;
    draw_toggle(
        fb,
        x_label,
        x_value,
        y,
        "Command",
        settings.contains(ScannerSettings::COMMAND),
        &label_style,
        &on_style,
        &off_style,
    );
    y += 50;

    Text::new("Mode:", Point::new(x_label, y), label_style)
        .draw(fb)
        .ok();
    let mode_str = if settings.contains(ScannerSettings::CONTINUOUS) {
        "CONTINUOUS (auto-scan)"
    } else if settings.contains(ScannerSettings::COMMAND) {
        "COMMAND (trigger needed)"
    } else {
        "MANUAL (button)"
    };
    Text::new(mode_str, Point::new(x_value, y), on_style)
        .draw(fb)
        .ok();
    y += 35;

    Text::new("Raw:", Point::new(x_label, y), label_style)
        .draw(fb)
        .ok();
    let mut hex = heapless::String::<8>::new();
    let _ = hex.push_str("0x");
    let _ = hex.push_str(&format_byte(settings.bits()));
    Text::new(&hex, Point::new(x_value, y), val_style)
        .draw(fb)
        .ok();
}

pub fn render_scan_result(fb: &mut impl DrawTarget<Color = Rgb565>, data: &[u8]) {
    let payload = gm65_scanner::decode_payload(data);
    render_decoded_scan(fb, &payload);
}

pub fn render_decoded_scan(fb: &mut impl DrawTarget<Color = Rgb565>, payload: &DecodedPayload) {
    let _ = fb.clear(Rgb565::BLACK);

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let label_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_YELLOW);
    let value_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let ok_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_GREEN);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style(
        "Scan Result",
        Point::new(DISPLAY_CENTER_X, 30),
        title_style,
        center_text,
    )
    .draw(fb)
    .ok();

    let tn = type_name(&payload.payload_type);
    Text::with_text_style(tn, Point::new(DISPLAY_CENTER_X, 60), ok_style, center_text)
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
            render_raw_data(fb, raw, y, &label_style, &value_style);
        }
        PayloadType::CashuV3 => {
            Text::new("Type:", Point::new(20, y as i32), label_style)
                .draw(fb)
                .ok();
            Text::new("Cashu V3 (legacy)", Point::new(120, y as i32), value_style)
                .draw(fb)
                .ok();
            y += 30;
            render_raw_data(fb, raw, y, &label_style, &value_style);
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
            render_raw_data(fb, raw, y, &label_style, &value_style);
        }
        PayloadType::Url | PayloadType::PlainText | PayloadType::Binary => {
            render_raw_data(fb, raw, y, &label_style, &value_style);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_toggle(
    fb: &mut impl DrawTarget<Color = Rgb565>,
    x_label: i32,
    x_value: i32,
    y: i32,
    name: &str,
    enabled: bool,
    label_style: &MonoTextStyle<'_, Rgb565>,
    on_style: &MonoTextStyle<'_, Rgb565>,
    off_style: &MonoTextStyle<'_, Rgb565>,
) {
    Text::new(name, Point::new(x_label, y), *label_style)
        .draw(fb)
        .ok();
    let (text, style) = if enabled {
        ("ON", *on_style)
    } else {
        ("OFF", *off_style)
    };
    Text::new(text, Point::new(x_value, y), style).draw(fb).ok();
}

fn render_raw_data(
    fb: &mut impl DrawTarget<Color = Rgb565>,
    raw: &[u8],
    start_y: u32,
    label_style: &MonoTextStyle<'_, Rgb565>,
    value_style: &MonoTextStyle<'_, Rgb565>,
) {
    let mut y = start_y;

    Text::new("Data:", Point::new(20, y as i32), *label_style)
        .draw(fb)
        .ok();
    y += 25;

    let data_str = core::str::from_utf8(raw).unwrap_or("<binary data>");
    let chars_per_line = 76;
    let mut offset = 0;
    while offset < data_str.len() && y < DISPLAY_MAX_Y - 20 {
        let end = core::cmp::min(offset + chars_per_line, data_str.len());
        Text::new(
            &data_str[offset..end],
            Point::new(20, y as i32),
            *value_style,
        )
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
