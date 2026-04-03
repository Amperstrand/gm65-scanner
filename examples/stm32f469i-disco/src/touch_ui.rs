use crate::compatibility::CompatibilityProfile;

pub const SETTINGS_START_Y: u16 = 80;
pub const SETTINGS_ROW_HEIGHT: u16 = 35;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityAction {
    CycleUsbMode,
    CycleSuffix,
    CycleKeyDelay,
    CycleCaseMode,
    ToggleFastHid,
    ToggleCapsOverride,
    ToggleSimulatedCapsLock,
}

pub fn map_touch_to_action(y: u16) -> Option<CompatibilityAction> {
    if y < SETTINGS_START_Y {
        return None;
    }

    match (y - SETTINGS_START_Y) / SETTINGS_ROW_HEIGHT {
        0 => Some(CompatibilityAction::CycleUsbMode),
        1 => Some(CompatibilityAction::CycleSuffix),
        2 => Some(CompatibilityAction::CycleKeyDelay),
        3 => Some(CompatibilityAction::CycleCaseMode),
        4 => Some(CompatibilityAction::ToggleFastHid),
        5 => Some(CompatibilityAction::ToggleCapsOverride),
        6 => Some(CompatibilityAction::ToggleSimulatedCapsLock),
        _ => None,
    }
}

pub fn apply_action(
    mut profile: CompatibilityProfile,
    action: CompatibilityAction,
) -> (CompatibilityProfile, bool) {
    let reboot = match action {
        CompatibilityAction::CycleUsbMode => {
            profile.usb_mode = profile.usb_mode.cycle();
            true
        }
        CompatibilityAction::CycleSuffix => {
            profile.suffix = profile.suffix.cycle();
            false
        }
        CompatibilityAction::CycleKeyDelay => {
            profile.cycle_key_delay();
            false
        }
        CompatibilityAction::CycleCaseMode => {
            profile.case_mode = profile.case_mode.cycle();
            false
        }
        CompatibilityAction::ToggleFastHid => {
            profile.fast_hid = !profile.fast_hid;
            true
        }
        CompatibilityAction::ToggleCapsOverride => {
            profile.caps_lock_override = !profile.caps_lock_override;
            false
        }
        CompatibilityAction::ToggleSimulatedCapsLock => {
            profile.simulated_caps_lock = !profile.simulated_caps_lock;
            false
        }
    };

    (profile, reboot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compatibility::{CaseMode, SuffixMode, UsbMode};

    #[test]
    fn touch_rows_map_to_expected_actions() {
        assert_eq!(
            map_touch_to_action(80),
            Some(CompatibilityAction::CycleUsbMode)
        );
        assert_eq!(
            map_touch_to_action(115),
            Some(CompatibilityAction::CycleSuffix)
        );
        assert_eq!(
            map_touch_to_action(290),
            Some(CompatibilityAction::ToggleSimulatedCapsLock)
        );
        assert_eq!(map_touch_to_action(20), None);
        assert_eq!(map_touch_to_action(400), None);
    }

    #[test]
    fn action_application_updates_profile() {
        let profile = CompatibilityProfile::default();
        let (profile, reboot) = apply_action(profile, CompatibilityAction::CycleUsbMode);
        assert_eq!(profile.usb_mode, UsbMode::Ds2208HidPos);
        assert!(reboot);

        let (profile, reboot) = apply_action(profile, CompatibilityAction::CycleSuffix);
        assert_eq!(profile.suffix, SuffixMode::Tab);
        assert!(!reboot);

        let (profile, _) = apply_action(profile, CompatibilityAction::CycleCaseMode);
        assert_eq!(profile.case_mode, CaseMode::Upper);
    }
}
