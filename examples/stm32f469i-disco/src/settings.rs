use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::Rectangle,
    text::{Alignment, Text, TextStyleBuilder},
};
use stm32f469i_disc::hal::ltdc::LtdcFramebuffer;

const FB_W: i32 = 480;
const FB_H: i32 = 800;
const BTN_W: u32 = 420;
const BTN_H: u32 = 70;
const BTN_GAP: u32 = 12;
const BTN_X: i32 = 30;
const BTN_START_Y: i32 = 100;

const COLOR_ON: Rgb565 = Rgb565::new(0x00, 0xA0, 0x00);
const COLOR_OFF: Rgb565 = Rgb565::new(0x30, 0x30, 0x30);
const COLOR_BG: Rgb565 = Rgb565::BLACK;
const COLOR_TITLE: Rgb565 = Rgb565::CSS_CYAN;
const COLOR_LABEL: Rgb565 = Rgb565::WHITE;
const COLOR_VALUE_ON: Rgb565 = Rgb565::new(0x40, 0xFF, 0x40);
const COLOR_VALUE_OFF: Rgb565 = Rgb565::new(0x80, 0x80, 0x80);
const COLOR_BACK: Rgb565 = Rgb565::new(0xA0, 0x30, 0x30);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SuffixKey {
    None,
    Enter,
    Tab,
}

impl SuffixKey {
    pub const VALUES: [SuffixKey; 3] = [SuffixKey::None, SuffixKey::Enter, SuffixKey::Tab];
    pub const LABELS: [&'static str; 3] = ["None", "Enter", "Tab"];

    pub fn as_str(&self) -> &'static str {
        match self {
            SuffixKey::None => "None",
            SuffixKey::Enter => "Enter",
            SuffixKey::Tab => "Tab",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            SuffixKey::None => SuffixKey::Enter,
            SuffixKey::Enter => SuffixKey::Tab,
            SuffixKey::Tab => SuffixKey::None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UsbMode {
    Composite,
    KeyboardOnly,
    PosOnly,
}

impl UsbMode {
    pub const VALUES: [UsbMode; 3] = [UsbMode::Composite, UsbMode::KeyboardOnly, UsbMode::PosOnly];
    pub const LABELS: [&'static str; 3] = ["All (CDC+KBD+POS)", "Keyboard only", "POS only"];

    pub fn as_str(&self) -> &'static str {
        match self {
            UsbMode::Composite => "All",
            UsbMode::KeyboardOnly => "KBD",
            UsbMode::PosOnly => "POS",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            UsbMode::Composite => UsbMode::KeyboardOnly,
            UsbMode::KeyboardOnly => UsbMode::PosOnly,
            UsbMode::PosOnly => UsbMode::Composite,
        }
    }
}

pub struct DeviceSettings {
    pub sound: bool,
    pub aim: bool,
    pub light: bool,
    pub command_mode: bool,
    pub usb_mode: UsbMode,
    pub suffix_key: SuffixKey,
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            sound: true,
            aim: true,
            light: false,
            command_mode: true,
            usb_mode: UsbMode::Composite,
            suffix_key: SuffixKey::Enter,
        }
    }
}

pub enum SettingsAction {
    None,
    Back,
    Apply,
}

enum ButtonKind {
    Toggle {
        field: &'static str,
        setting: fn(&DeviceSettings) -> bool,
        apply: fn(&mut DeviceSettings, bool),
    },
    Cycle {
        label: &'static str,
        values: CycleValues,
    },
}

enum CycleValues {
    UsbMode,
    SuffixKey,
}

impl CycleValues {
    fn label(&self, settings: &DeviceSettings) -> &'static str {
        match self {
            CycleValues::UsbMode => settings.usb_mode.as_str(),
            CycleValues::SuffixKey => settings.suffix_key.as_str(),
        }
    }

    fn next(&self, settings: &mut DeviceSettings) {
        match self {
            CycleValues::UsbMode => settings.usb_mode = settings.usb_mode.next(),
            CycleValues::SuffixKey => settings.suffix_key = settings.suffix_key.next(),
        }
    }
}

struct Button {
    kind: ButtonKind,
    y: i32,
}

impl Button {
    fn bounds(&self) -> Rectangle {
        Rectangle::new(Point::new(BTN_X, self.y), Size::new(BTN_W, BTN_H))
    }

    fn hit_test(&self, tx: u16, ty: u16) -> bool {
        let b = self.bounds();
        let x = tx as i32;
        let y = ty as i32;
        x >= b.top_left.x
            && x < b.top_left.x + b.size.width as i32
            && y >= b.top_left.y
            && y < b.top_left.y + b.size.height as i32
    }
}

pub struct SettingsScreen {
    buttons: [Button; 7],
    back_bounds: Rectangle,
}

impl SettingsScreen {
    pub fn new() -> Self {
        let buttons = [
            Button {
                kind: ButtonKind::Toggle {
                    field: "Sound",
                    setting: |s| s.sound,
                    apply: |s, v| s.sound = v,
                },
                y: btn_y(0),
            },
            Button {
                kind: ButtonKind::Toggle {
                    field: "Aim (laser)",
                    setting: |s| s.aim,
                    apply: |s, v| s.aim = v,
                },
                y: btn_y(1),
            },
            Button {
                kind: ButtonKind::Toggle {
                    field: "Light (illum)",
                    setting: |s| s.light,
                    apply: |s, v| s.light = v,
                },
                y: btn_y(2),
            },
            Button {
                kind: ButtonKind::Toggle {
                    field: "Command mode",
                    setting: |s| s.command_mode,
                    apply: |s, v| s.command_mode = v,
                },
                y: btn_y(3),
            },
            Button {
                kind: ButtonKind::Cycle {
                    label: "USB Mode",
                    values: CycleValues::UsbMode,
                },
                y: btn_y(4),
            },
            Button {
                kind: ButtonKind::Cycle {
                    label: "Suffix Key",
                    values: CycleValues::SuffixKey,
                },
                y: btn_y(5),
            },
            Button {
                kind: ButtonKind::Cycle {
                    label: "Baud Rate",
                    values: CycleValues::SuffixKey,
                },
                y: btn_y(6),
            },
        ];

        let back_y = FB_H as i32 - 100;
        let back_bounds = Rectangle::new(Point::new(BTN_X, back_y), Size::new(BTN_W, 60));

        Self {
            buttons,
            back_bounds,
        }
    }

    pub fn draw(&self, fb: &mut LtdcFramebuffer<u16>, settings: &DeviceSettings) {
        fb.clear(COLOR_BG).ok();

        let title_style = MonoTextStyle::new(&FONT_10X20, COLOR_TITLE);
        let center = TextStyleBuilder::new().alignment(Alignment::Center).build();
        Text::with_text_style("Settings", Point::new(FB_W / 2, 30), title_style, center)
            .draw(fb)
            .ok();

        let sep = Rectangle::new(Point::new(20, 65), Size::new(FB_W as u32 - 40, 1));
        fb.fill_solid(&sep, Rgb565::new(0x40, 0x40, 0x40)).ok();

        for btn in &self.buttons {
            match &btn.kind {
                ButtonKind::Toggle { field, setting, .. } => {
                    let on = setting(settings);
                    let bg = if on { COLOR_ON } else { COLOR_OFF };
                    fb.fill_solid(&btn.bounds(), bg).ok();

                    let label_style = MonoTextStyle::new(&FONT_10X20, COLOR_LABEL);
                    Text::new(*field, Point::new(BTN_X + 15, btn.y + 22), label_style)
                        .draw(fb)
                        .ok();

                    let val_str = if on { "ON" } else { "OFF" };
                    let val_color = if on { COLOR_VALUE_ON } else { COLOR_VALUE_OFF };
                    let val_style = MonoTextStyle::new(&FONT_10X20, val_color);
                    let val_x = BTN_X + BTN_W as i32 - 80;
                    Text::new(val_str, Point::new(val_x, btn.y + 22), val_style)
                        .draw(fb)
                        .ok();
                }
                ButtonKind::Cycle { label, values } => {
                    fb.fill_solid(&btn.bounds(), COLOR_OFF).ok();

                    let label_style = MonoTextStyle::new(&FONT_10X20, COLOR_LABEL);
                    Text::new(*label, Point::new(BTN_X + 15, btn.y + 22), label_style)
                        .draw(fb)
                        .ok();

                    let val_str = values.label(settings);
                    let val_style = MonoTextStyle::new(&FONT_10X20, COLOR_TITLE);
                    let val_x = BTN_X + BTN_W as i32 - 80;
                    Text::new(val_str, Point::new(val_x, btn.y + 22), val_style)
                        .draw(fb)
                        .ok();
                }
            }
        }

        fb.fill_solid(&self.back_bounds, COLOR_BACK).ok();
        let back_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
        let back_center = TextStyleBuilder::new().alignment(Alignment::Center).build();
        Text::with_text_style(
            "< Back to Scanner",
            Point::new(BTN_X + BTN_W as i32 / 2, self.back_bounds.top_left.y + 18),
            back_style,
            back_center,
        )
        .draw(fb)
        .ok();
    }

    pub fn handle_touch(&self, tx: u16, ty: u16, settings: &mut DeviceSettings) -> SettingsAction {
        if self.back_bounds.contains(Point::new(tx as i32, ty as i32)) {
            return SettingsAction::Back;
        }

        for btn in &self.buttons {
            if btn.hit_test(tx, ty) {
                match &btn.kind {
                    ButtonKind::Toggle { setting, apply, .. } => {
                        let new_val = !setting(settings);
                        apply(settings, new_val);
                        return SettingsAction::Apply;
                    }
                    ButtonKind::Cycle { values, .. } => {
                        values.next(settings);
                        return SettingsAction::Apply;
                    }
                }
            }
        }

        SettingsAction::None
    }
}

const fn btn_y(index: usize) -> i32 {
    BTN_START_Y + (index as i32) * (BTN_H as i32 + BTN_GAP as i32)
}
