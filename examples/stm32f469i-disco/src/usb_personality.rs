use crate::compatibility::{CompatibilityProfile, UsbMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsbPersonalityInfo {
    pub product: &'static str,
    pub serial: &'static str,
    pub poll_ms: u8,
}

pub const fn info_for_profile(profile: CompatibilityProfile) -> UsbPersonalityInfo {
    let (product, serial) = match profile.usb_mode {
        UsbMode::AdminCdc => ("GM65 Admin CDC", "f469disco-admin"),
        UsbMode::Ds2208KeyboardHid => ("GM65 DS2208-Compatible Keyboard", "f469disco-kbd"),
        UsbMode::Ds2208HidPos => ("GM65 DS2208-Compatible POS", "f469disco-pos"),
    };

    UsbPersonalityInfo {
        product,
        serial,
        poll_ms: if profile.fast_hid { 1 } else { 10 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compatibility::{CompatibilityProfile, UsbMode};

    #[test]
    fn usb_personality_metadata_matches_mode() {
        let keyboard = info_for_profile(CompatibilityProfile::default());
        assert_eq!(keyboard.product, "GM65 DS2208-Compatible Keyboard");
        assert_eq!(keyboard.serial, "f469disco-kbd");
        assert_eq!(keyboard.poll_ms, 1);

        let pos = info_for_profile(CompatibilityProfile {
            usb_mode: UsbMode::Ds2208HidPos,
            fast_hid: false,
            ..CompatibilityProfile::default()
        });
        assert_eq!(pos.product, "GM65 DS2208-Compatible POS");
        assert_eq!(pos.serial, "f469disco-pos");
        assert_eq!(pos.poll_ms, 10);
    }
}
