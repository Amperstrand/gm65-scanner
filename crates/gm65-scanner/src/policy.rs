//! Scan policies: the blessed module configurations, so consumers never
//! compose raw SETTINGS values (the 2026-09-11 micronuts integration beeped
//! the lab by writing 0xD2 — buzzer is bit 6 — while composing "continuous"
//! by hand).
//!
//! `start_scanning(policy)` on either driver applies the policy with the
//! proven sequence: stop, write SETTINGS, then write ScanEnable=1 (the
//! module needs the start signal; bench lesson 68dd5e2).

use crate::settings::{AimSetting, LightSetting, ReadMode, ScannerSettings};

/// The blessed scan configurations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanPolicy {
    /// Continuous scanning, aim LED while reading, decode buzzer OFF.
    /// SETTINGS 0x92 — the proven bench configuration for hosts that read
    /// scans as they stream.
    SilentContinuous,
    /// Continuous scanning with the decode buzzer armed (SETTINGS 0xD2) —
    /// for rig pose-finding by ear (issue #99).
    BuzzingContinuous,
    /// Command-triggered, silent (SETTINGS 0x91) — for hosts that poll
    /// ScanEnable per scan. Beware issue #75: some module firmwares ACK
    /// the trigger in this mode without ever scanning; prefer a
    /// continuous policy unless you have verified trigger behavior.
    SilentCommand,
}

impl ScanPolicy {
    /// The SETTINGS register value this policy writes.
    #[must_use]
    pub fn settings(self) -> ScannerSettings {
        let mut s = ScannerSettings::default();
        match self {
            Self::SilentContinuous => s.read_mode = ReadMode::Continuous,
            Self::BuzzingContinuous => {
                s.read_mode = ReadMode::Continuous;
                s.buzzer = true;
            }
            Self::SilentCommand => {}
        }
        s
    }

    /// The SETTINGS register byte this policy writes.
    #[must_use]
    pub fn settings_byte(self) -> u8 {
        self.settings().bits()
    }
}

/// Convenience for consumers that manage SETTINGS by hand: the aim/light
/// fields the blessed policies use.
#[must_use]
pub fn bench_baseline_settings() -> ScannerSettings {
    ScannerSettings {
        always_on: true,
        buzzer: false,
        aim: AimSetting::Reading,
        light: LightSetting::Off,
        read_mode: ReadMode::Continuous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_bytes_match_bench_verified_values() {
        assert_eq!(ScanPolicy::SilentContinuous.settings_byte(), 0x92);
        assert_eq!(ScanPolicy::BuzzingContinuous.settings_byte(), 0xD2);
        assert_eq!(ScanPolicy::SilentCommand.settings_byte(), 0x91);
    }

    #[test]
    fn only_the_buzzer_bit_differs_between_continuous_policies() {
        assert_eq!(
            ScanPolicy::SilentContinuous.settings_byte() | 0x40,
            ScanPolicy::BuzzingContinuous.settings_byte()
        );
    }

    #[test]
    fn baseline_is_silent_continuous() {
        assert_eq!(bench_baseline_settings().bits(), 0x92);
    }
}
