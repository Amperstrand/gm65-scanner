use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    primitives::Rectangle,
    text::{Alignment, Text, TextStyleBuilder},
};

use gm65_scanner::{DecodedPayload, PayloadType, ScannerSettings};

use crate::display_utils::{format_byte, format_u32_len, truncate_str};

mod theme {
    use embedded_graphics::pixelcolor::Rgb888;
    pub const BG_DARK: Rgb888 = Rgb888::new(0x18, 0x18, 0x18);
    pub const ACCENT_CYAN: Rgb888 = Rgb888::new(0x00, 0xFF, 0xFF);
    pub const SUCCESS: Rgb888 = Rgb888::new(0x00, 0xFF, 0x00);
    pub const ERROR: Rgb888 = Rgb888::new(0xFF, 0x00, 0x00);
    pub const TEXT_PRIMARY: Rgb888 = Rgb888::new(0xFF, 0xFF, 0xFF);
    pub const _TEXT_SECONDARY: Rgb888 = Rgb888::new(0xA0, 0xA0, 0xA0);
}

const Y_HOME_TITLE: i32 = 80;
const Y_HOME_READY: i32 = 120;
const Y_HOME_SCANNER_ROW: i32 = 200;
const Y_HOME_HELP: i32 = 500;
const Y_PAGE_TITLE: i32 = 30;
const Y_ERROR_TITLE: i32 = 200;
const Y_MESSAGE: i32 = 240;
const Y_SETTINGS_START: i32 = 150;
const Y_RESULT_TYPE_NAME: i32 = 60;
const Y_RESULT_START: i32 = 100;

const X_LABEL: i32 = 20;
const X_MODEL_VALUE: i32 = 140;
const X_RESULT_VALUE: i32 = 120;
const X_PROGRESS_VALUE: i32 = 160;
const X_SETTINGS_VALUE: i32 = 200;

const ROW_SPACING: i32 = 90;
const ROW_BG_X_OFFSET: i32 = 10;
const ROW_BG_Y_OFFSET: i32 = -30;
const ROW_BG_WIDTH: u32 = 460;
const ROW_BG_HEIGHT: u32 = 60;
const ROW_HEIGHT: i32 = 30;

const BTN_SETTINGS_X: i32 = 130;
const BTN_SETTINGS_Y: i32 = 670;
const BTN_SETTINGS_W: u32 = 220;
const BTN_SETTINGS_H: u32 = 60;
const Y_SETTINGS_LABEL: i32 = 710;

const BTN_BACK_X: i32 = 40;
const BTN_BACK_Y: i32 = 715;
const BTN_BACK_W: u32 = 200;
const BTN_BACK_H: u32 = 50;
const X_BACK_LABEL: i32 = 60;
const Y_BACK_LABEL: i32 = 750;

const MAX_MESSAGE_LEN: usize = 60;
const HEX_BUF_SIZE: usize = 8;
const CHARS_PER_LINE: usize = 76;
const DATA_LINE_HEIGHT: i32 = 22;

const MODE_LABEL_Y_OFFSET: i32 = 35;
const DATA_LABEL_Y_OFFSET: i32 = 25;
const BOTTOM_MARGIN: i32 = 20;

pub fn render_status(fb: &mut impl DrawTarget<Color = Rgb888>, message: &str) {
    let _ = fb.clear(Rgb888::BLACK);
    let style = MonoTextStyle::new(&FONT_10X20, theme::TEXT_PRIMARY);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(
        truncate_str(message, MAX_MESSAGE_LEN),
        Point::new(DISPLAY_CENTER_X, Y_MESSAGE),
        style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_home(fb: &mut impl DrawTarget<Color = Rgb888>, scanner_connected: bool, model: &str) {
    let _ = fb.clear(Rgb888::BLACK);

    let title_style = MonoTextStyle::new(&FONT_10X20, theme::ACCENT_CYAN);
    let style = MonoTextStyle::new(&FONT_10X20, theme::TEXT_PRIMARY);
    let ok_style = MonoTextStyle::new(&FONT_10X20, theme::SUCCESS);
    let err_style = MonoTextStyle::new(&FONT_10X20, theme::ERROR);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style(
        "QR Scanner",
        Point::new(DISPLAY_CENTER_X, Y_HOME_TITLE),
        title_style,
        center_text,
    )
    .draw(fb)
    .ok();

    Text::with_text_style(
        "Ready",
        Point::new(DISPLAY_CENTER_X, Y_HOME_READY),
        style,
        center_text,
    )
    .draw(fb)
    .ok();

    Text::new("Scanner:", Point::new(X_LABEL, Y_HOME_SCANNER_ROW), style)
        .draw(fb)
        .ok();

    if scanner_connected {
        Text::new(
            model,
            Point::new(X_MODEL_VALUE, Y_HOME_SCANNER_ROW),
            ok_style,
        )
        .draw(fb)
        .ok();
    } else {
        Text::new(
            "NOT FOUND",
            Point::new(X_MODEL_VALUE, Y_HOME_SCANNER_ROW),
            err_style,
        )
        .draw(fb)
        .ok();
    }

    Text::with_text_style(
        "Scan a QR code or send USB command...",
        Point::new(DISPLAY_CENTER_X, Y_HOME_HELP),
        style,
        center_text,
    )
    .draw(fb)
    .ok();

    fb.fill_solid(
        &Rectangle::new(
            Point::new(BTN_SETTINGS_X, BTN_SETTINGS_Y),
            Size::new(BTN_SETTINGS_W, BTN_SETTINGS_H),
        ),
        theme::BG_DARK,
    )
    .ok();

    Text::with_text_style(
        "Settings",
        Point::new(DISPLAY_CENTER_X, Y_SETTINGS_LABEL),
        title_style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_error(fb: &mut impl DrawTarget<Color = Rgb888>, message: &str) {
    let _ = fb.clear(Rgb888::BLACK);
    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb888::RED);
    let msg_style = MonoTextStyle::new(&FONT_10X20, theme::TEXT_PRIMARY);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();
    Text::with_text_style(
        "ERROR",
        Point::new(DISPLAY_CENTER_X, Y_ERROR_TITLE),
        title_style,
        center_text,
    )
    .draw(fb)
    .ok();
    Text::with_text_style(
        truncate_str(message, MAX_MESSAGE_LEN),
        Point::new(DISPLAY_CENTER_X, Y_MESSAGE),
        msg_style,
        center_text,
    )
    .draw(fb)
    .ok();
}

pub fn render_scanner_settings(
    fb: &mut impl DrawTarget<Color = Rgb888>,
    settings: ScannerSettings,
) {
    let _ = fb.clear(Rgb888::BLACK);

    let title_style = MonoTextStyle::new(&FONT_10X20, theme::ACCENT_CYAN);
    let label_style = MonoTextStyle::new(&FONT_10X20, Rgb888::new(255, 255, 0));
    let on_style = MonoTextStyle::new(&FONT_10X20, theme::SUCCESS);
    let off_style = MonoTextStyle::new(&FONT_10X20, theme::ERROR);
    let val_style = MonoTextStyle::new(&FONT_10X20, theme::TEXT_PRIMARY);

    Text::with_text_style(
        "Scanner Settings",
        Point::new(DISPLAY_CENTER_X, Y_PAGE_TITLE),
        title_style,
        TextStyleBuilder::new().alignment(Alignment::Center).build(),
    )
    .draw(fb)
    .ok();

    let mut y = Y_SETTINGS_START;
    let x_label = X_LABEL;
    let x_value = X_SETTINGS_VALUE;

    fn draw_row_bg<D: DrawTarget<Color = Rgb888>>(fb: &mut D, y: i32) {
        fb.fill_solid(
            &Rectangle::new(
                Point::new(ROW_BG_X_OFFSET, y + ROW_BG_Y_OFFSET),
                Size::new(ROW_BG_WIDTH, ROW_BG_HEIGHT),
            ),
            theme::BG_DARK,
        )
        .ok();
    }

    draw_row_bg(fb, y);
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
    y += ROW_SPACING;
    draw_row_bg(fb, y);
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
    y += ROW_SPACING;
    draw_row_bg(fb, y);
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
    y += ROW_SPACING;
    draw_row_bg(fb, y);
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
    y += ROW_SPACING;
    draw_row_bg(fb, y);
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
    y += ROW_SPACING;

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
    y += MODE_LABEL_Y_OFFSET;

    Text::new("Raw:", Point::new(x_label, y), label_style)
        .draw(fb)
        .ok();
    let mut hex = heapless::String::<HEX_BUF_SIZE>::new();
    let _ = hex.push_str("0x");
    let _ = hex.push_str(&format_byte(settings.bits()));
    Text::new(&hex, Point::new(x_value, y), val_style)
        .draw(fb)
        .ok();

    fb.fill_solid(
        &Rectangle::new(
            Point::new(BTN_BACK_X, BTN_BACK_Y),
            Size::new(BTN_BACK_W, BTN_BACK_H),
        ),
        theme::BG_DARK,
    )
    .ok();

    Text::new("< Back", Point::new(X_BACK_LABEL, Y_BACK_LABEL), on_style)
        .draw(fb)
        .ok();
}

pub fn render_scan_result(fb: &mut impl DrawTarget<Color = Rgb888>, data: &[u8]) {
    let payload = gm65_scanner::decode_payload(data);
    render_decoded_scan(fb, &payload);
}

pub fn render_decoded_scan(fb: &mut impl DrawTarget<Color = Rgb888>, payload: &DecodedPayload) {
    let _ = fb.clear(Rgb888::BLACK);

    let title_style = MonoTextStyle::new(&FONT_10X20, theme::ACCENT_CYAN);
    let label_style = MonoTextStyle::new(&FONT_10X20, Rgb888::new(255, 255, 0));
    let value_style = MonoTextStyle::new(&FONT_10X20, theme::TEXT_PRIMARY);
    let ok_style = MonoTextStyle::new(&FONT_10X20, theme::SUCCESS);
    let center_text = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style(
        "Scan Result",
        Point::new(DISPLAY_CENTER_X, Y_PAGE_TITLE),
        title_style,
        center_text,
    )
    .draw(fb)
    .ok();

    let tn = type_name(&payload.payload_type);
    Text::with_text_style(
        tn,
        Point::new(DISPLAY_CENTER_X, Y_RESULT_TYPE_NAME),
        ok_style,
        center_text,
    )
    .draw(fb)
    .ok();

    let raw = &payload.raw;
    let len_label = format_u32_len(raw.len());

    let mut y = Y_RESULT_START as u32;

    Text::new("Size:", Point::new(X_LABEL, y as i32), label_style)
        .draw(fb)
        .ok();
    Text::new(
        &len_label,
        Point::new(X_RESULT_VALUE, y as i32),
        value_style,
    )
    .draw(fb)
    .ok();
    y += ROW_HEIGHT as u32;

    match payload.payload_type {
        PayloadType::CashuV4 => {
            Text::new("Type:", Point::new(X_LABEL, y as i32), label_style)
                .draw(fb)
                .ok();
            Text::new(
                "Cashu V4 Token",
                Point::new(X_RESULT_VALUE, y as i32),
                value_style,
            )
            .draw(fb)
            .ok();
            y += ROW_HEIGHT as u32;
            render_raw_data(fb, raw, y, &label_style, &value_style);
        }
        PayloadType::CashuV3 => {
            Text::new("Type:", Point::new(X_LABEL, y as i32), label_style)
                .draw(fb)
                .ok();
            Text::new(
                "Cashu V3 (legacy)",
                Point::new(X_RESULT_VALUE, y as i32),
                value_style,
            )
            .draw(fb)
            .ok();
            y += ROW_HEIGHT as u32;
            render_raw_data(fb, raw, y, &label_style, &value_style);
        }
        PayloadType::UrFragment => {
            if let Some(parsed) = gm65_scanner::parse_ur_fragment(raw) {
                let mut frag_str = heapless::String::<32>::new();
                let _ = frag_str.push_str(&format_u32_len(parsed.index as usize));
                let _ = frag_str.push('/');
                let _ = frag_str.push_str(&format_u32_len(parsed.total as usize));
                Text::new("Progress:", Point::new(X_LABEL, y as i32), label_style)
                    .draw(fb)
                    .ok();
                Text::new(
                    &frag_str,
                    Point::new(X_PROGRESS_VALUE, y as i32),
                    value_style,
                )
                .draw(fb)
                .ok();
                y += ROW_HEIGHT as u32;

                let mut type_str = heapless::String::<32>::new();
                let _ = type_str.push_str("UR Type: ");
                let _ = type_str.push_str(&parsed.ur_type);
                Text::new(&type_str, Point::new(X_LABEL, y as i32), label_style)
                    .draw(fb)
                    .ok();
                y += ROW_HEIGHT as u32;
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
    fb: &mut impl DrawTarget<Color = Rgb888>,
    x_label: i32,
    x_value: i32,
    y: i32,
    name: &str,
    enabled: bool,
    label_style: &MonoTextStyle<'_, Rgb888>,
    on_style: &MonoTextStyle<'_, Rgb888>,
    off_style: &MonoTextStyle<'_, Rgb888>,
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
    fb: &mut impl DrawTarget<Color = Rgb888>,
    raw: &[u8],
    start_y: u32,
    label_style: &MonoTextStyle<'_, Rgb888>,
    value_style: &MonoTextStyle<'_, Rgb888>,
) {
    let mut y = start_y;

    Text::new("Data:", Point::new(X_LABEL, y as i32), *label_style)
        .draw(fb)
        .ok();
    y += DATA_LABEL_Y_OFFSET as u32;

    let data_str = core::str::from_utf8(raw).unwrap_or("<binary data>");
    let mut offset = 0;
    while offset < data_str.len() && y < DISPLAY_MAX_Y - BOTTOM_MARGIN as u32 {
        let end = core::cmp::min(offset + CHARS_PER_LINE, data_str.len());
        Text::new(
            &data_str[offset..end],
            Point::new(X_LABEL, y as i32),
            *value_style,
        )
        .draw(fb)
        .ok();
        offset = end;
        y += DATA_LINE_HEIGHT as u32;
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
