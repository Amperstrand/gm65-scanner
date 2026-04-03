use gm65_scanner::hid::keyboard::{
    HidKeyboardReport, KeyMapper, Terminator, KEY_CAPS_LOCK, US_ENGLISH,
};

use crate::compatibility::{CaseMode, CompatibilityProfile, SuffixMode};

pub fn profile_terminator(mode: SuffixMode) -> Terminator {
    match mode {
        SuffixMode::None => Terminator::None,
        SuffixMode::Enter => Terminator::Enter,
        SuffixMode::Tab => Terminator::Tab,
    }
}

fn send_caps_toggle_report_sequence<const N: usize>(out: &mut heapless::Vec<[u8; 8], N>) -> bool {
    out.push(HidKeyboardReport::press(0, KEY_CAPS_LOCK).as_bytes())
        .is_ok()
        && out.push(HidKeyboardReport::release().as_bytes()).is_ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardBuildStats {
    pub skipped_bytes: usize,
    pub simulated_caps_wrap: bool,
}

/// The async firmware applies the optional key-delay after release reports only.
///
/// This preserves a normal press/release pair while adding spacing between
/// emitted keystrokes (including configured suffix-key reports). In the boot
/// keyboard report format used here, `report[2] == 0` means no keycode is
/// pressed, which corresponds to the release half of an emitted keystroke.
pub const fn should_apply_key_delay(report: [u8; 8]) -> bool {
    report[2] == 0
}

pub fn build_keyboard_reports<const N: usize>(
    profile: CompatibilityProfile,
    caps_lock_on: bool,
    data: &[u8],
    out: &mut heapless::Vec<[u8; 8], N>,
) -> KeyboardBuildStats {
    out.clear();

    let mapper = KeyMapper::new(&US_ENGLISH, profile_terminator(profile.suffix));
    let mut skipped = 0usize;
    let mut wrapped_caps = false;
    let mut effective_caps = caps_lock_on;

    let has_alpha = data.iter().any(|b| b.is_ascii_alphabetic())
        || profile
            .prefix_slice()
            .iter()
            .any(|b| b.is_ascii_alphabetic())
        || profile
            .suffix_bytes_slice()
            .iter()
            .any(|b| b.is_ascii_alphabetic());

    if profile.simulated_caps_lock && has_alpha {
        let desired_caps = match profile.case_mode {
            CaseMode::Upper => true,
            CaseMode::Lower => false,
            CaseMode::Preserve => caps_lock_on,
        };
        if desired_caps != caps_lock_on && send_caps_toggle_report_sequence(out) {
            wrapped_caps = true;
            effective_caps = desired_caps;
        }
    }

    for raw in profile
        .prefix_slice()
        .iter()
        .copied()
        .chain(data.iter().copied())
        .chain(profile.suffix_bytes_slice().iter().copied())
    {
        let transformed = profile.transform_ascii(raw);
        match mapper.map_byte(transformed) {
            Some(mut report) => {
                if profile.caps_lock_override && effective_caps && transformed.is_ascii_alphabetic()
                {
                    report.modifier ^= 0x02;
                }
                if out.push(report.as_bytes()).is_err()
                    || out.push(HidKeyboardReport::release().as_bytes()).is_err()
                {
                    break;
                }
            }
            None => skipped += 1,
        }
    }

    for report in mapper.map_to_reports(b"") {
        if out.push(report.as_bytes()).is_err() {
            break;
        }
    }

    if wrapped_caps {
        let _ = send_caps_toggle_report_sequence(out);
    }

    KeyboardBuildStats {
        skipped_bytes: skipped,
        simulated_caps_wrap: wrapped_caps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compatibility::{CompatibilityProfile, UsbMode};
    use gm65_scanner::hid::keyboard::{KEY_ENTER, KEY_TAB};

    fn keycodes(reports: &[[u8; 8]]) -> heapless::Vec<u8, 32> {
        let mut out = heapless::Vec::new();
        for report in reports {
            if report[2] != 0 {
                let _ = out.push(report[2]);
            }
        }
        out
    }

    #[test]
    fn builds_reports_for_simple_scan() {
        let profile = CompatibilityProfile::default();
        let mut reports = heapless::Vec::<[u8; 8], 64>::new();

        let stats = build_keyboard_reports(profile, false, b"Ab", &mut reports);

        assert_eq!(stats.skipped_bytes, 0);
        assert!(!reports.is_empty());
        assert_eq!(reports[0][2], 4);
    }

    #[test]
    fn skips_unmappable_bytes_deterministically() {
        let profile = CompatibilityProfile::default();
        let mut reports = heapless::Vec::<[u8; 8], 64>::new();

        let stats = build_keyboard_reports(profile, false, &[0xff, b'A'], &mut reports);

        assert_eq!(stats.skipped_bytes, 1);
        assert!(reports.iter().any(|r| r[2] == 4));
    }

    #[test]
    fn simulated_caps_wraps_when_requested() {
        let profile = CompatibilityProfile {
            usb_mode: UsbMode::Ds2208KeyboardHid,
            case_mode: CaseMode::Upper,
            simulated_caps_lock: true,
            ..CompatibilityProfile::default()
        };
        let mut reports = heapless::Vec::<[u8; 8], 64>::new();

        let stats = build_keyboard_reports(profile, false, b"abc", &mut reports);

        assert!(stats.simulated_caps_wrap);
        assert!(reports.len() >= 4);
        assert_eq!(reports[0][2], 0x39);
    }

    #[test]
    fn suffix_none_emits_no_terminator() {
        let profile = CompatibilityProfile {
            suffix: SuffixMode::None,
            ..CompatibilityProfile::default()
        };
        let mut reports = heapless::Vec::<[u8; 8], 64>::new();

        build_keyboard_reports(profile, false, b"A", &mut reports);

        assert_eq!(
            keycodes(&reports),
            heapless::Vec::<u8, 32>::from_slice(&[4]).unwrap()
        );
    }

    #[test]
    fn suffix_enter_and_tab_emit_expected_terminators() {
        let mut enter_reports = heapless::Vec::<[u8; 8], 64>::new();
        let mut tab_reports = heapless::Vec::<[u8; 8], 64>::new();

        build_keyboard_reports(
            CompatibilityProfile {
                suffix: SuffixMode::Enter,
                ..CompatibilityProfile::default()
            },
            false,
            b"A",
            &mut enter_reports,
        );
        build_keyboard_reports(
            CompatibilityProfile {
                suffix: SuffixMode::Tab,
                ..CompatibilityProfile::default()
            },
            false,
            b"A",
            &mut tab_reports,
        );

        assert_eq!(
            keycodes(&enter_reports),
            heapless::Vec::<u8, 32>::from_slice(&[4, KEY_ENTER]).unwrap()
        );
        assert_eq!(
            keycodes(&tab_reports),
            heapless::Vec::<u8, 32>::from_slice(&[4, KEY_TAB]).unwrap()
        );
    }

    #[test]
    fn prefix_and_suffix_bytes_wrap_payload() {
        let mut profile = CompatibilityProfile::default();
        profile.suffix = SuffixMode::None;
        profile.set_prefix(b"[");
        profile.set_suffix_bytes(b"]");
        let mut reports = heapless::Vec::<[u8; 8], 64>::new();

        build_keyboard_reports(profile, false, b"A", &mut reports);

        assert_eq!(
            keycodes(&reports),
            heapless::Vec::<u8, 32>::from_slice(&[0x2f, 4, 0x30]).unwrap()
        );
    }

    #[test]
    fn upper_and_lower_case_modes_transform_ascii() {
        let mut upper_reports = heapless::Vec::<[u8; 8], 64>::new();
        let mut lower_reports = heapless::Vec::<[u8; 8], 64>::new();

        let upper = CompatibilityProfile {
            suffix: SuffixMode::None,
            case_mode: CaseMode::Upper,
            ..CompatibilityProfile::default()
        };
        let lower = CompatibilityProfile {
            suffix: SuffixMode::None,
            case_mode: CaseMode::Lower,
            ..CompatibilityProfile::default()
        };

        build_keyboard_reports(upper, false, b"a", &mut upper_reports);
        build_keyboard_reports(lower, false, b"A", &mut lower_reports);

        assert_eq!(upper_reports[0][2], 4);
        assert_eq!(upper_reports[0][0], 0x02);
        assert_eq!(lower_reports[0][2], 4);
        assert_eq!(lower_reports[0][0], 0x00);
    }

    #[test]
    fn caps_override_tracks_host_led_state() {
        let profile = CompatibilityProfile {
            suffix: SuffixMode::None,
            caps_lock_override: true,
            ..CompatibilityProfile::default()
        };
        let mut lower_reports = heapless::Vec::<[u8; 8], 64>::new();
        let mut upper_reports = heapless::Vec::<[u8; 8], 64>::new();

        build_keyboard_reports(profile, true, b"a", &mut lower_reports);
        build_keyboard_reports(profile, true, b"A", &mut upper_reports);

        assert_eq!(lower_reports[0][0], 0x02);
        assert_eq!(upper_reports[0][0], 0x00);
    }

    #[test]
    fn key_delay_applies_to_release_reports_only() {
        assert!(!should_apply_key_delay([0x02, 0, 4, 0, 0, 0, 0, 0]));
        assert!(should_apply_key_delay([0, 0, 0, 0, 0, 0, 0, 0]));
    }
}
