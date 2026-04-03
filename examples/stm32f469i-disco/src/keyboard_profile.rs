use gm65_scanner::hid::keyboard::{HidKeyboardReport, KeyMapper, Terminator, US_ENGLISH};

use crate::compatibility::{CaseMode, CompatibilityProfile, SuffixMode};

pub fn profile_terminator(mode: SuffixMode) -> Terminator {
    match mode {
        SuffixMode::None => Terminator::None,
        SuffixMode::Enter => Terminator::Enter,
        SuffixMode::Tab => Terminator::Tab,
    }
}

fn send_caps_toggle_report_sequence<const N: usize>(out: &mut heapless::Vec<[u8; 8], N>) -> bool {
    const KEY_CAPSLOCK: u8 = 0x39;
    out.push(HidKeyboardReport::press(0, KEY_CAPSLOCK).as_bytes())
        .is_ok()
        && out.push(HidKeyboardReport::release().as_bytes()).is_ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardBuildStats {
    pub skipped_bytes: usize,
    pub simulated_caps_wrap: bool,
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
}
