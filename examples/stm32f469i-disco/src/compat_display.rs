use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    text::{Alignment, Text, TextStyleBuilder},
};

use crate::compatibility::CompatibilityProfile;

pub fn render_compatibility_profile(
    fb: &mut impl DrawTarget<Color = Rgb565>,
    profile: CompatibilityProfile,
) {
    let _ = fb.clear(Rgb565::BLACK);

    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_CYAN);
    let label_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_YELLOW);
    let value_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let hint_style = MonoTextStyle::new(&FONT_10X20, Rgb565::CSS_GREEN);
    let center = TextStyleBuilder::new().alignment(Alignment::Center).build();

    Text::with_text_style(
        "DS2208 Compatibility",
        Point::new(240, 30),
        title_style,
        center,
    )
    .draw(fb)
    .ok();

    let rows = [
        ("USB Mode", profile.usb_mode.label()),
        ("Suffix", profile.suffix.label()),
        ("Key Delay", profile.key_delay_label()),
        ("Case", profile.case_mode.label()),
        ("Fast HID", if profile.fast_hid { "On" } else { "Off" }),
        (
            "Caps Override",
            if profile.caps_lock_override { "On" } else { "Off" },
        ),
        (
            "Sim Caps",
            if profile.simulated_caps_lock { "On" } else { "Off" },
        ),
    ];

    let mut y = 90;
    for (label, value) in rows {
        Text::new(label, Point::new(20, y), label_style).draw(fb).ok();
        Text::new(value, Point::new(220, y), value_style).draw(fb).ok();
        y += 35;
    }

    Text::with_text_style(
        "Tap rows to cycle. USB/Fast HID changes reboot.",
        Point::new(240, 380),
        hint_style,
        center,
    )
    .draw(fb)
    .ok();
}
