//! GM65 scanner configuration constants and settings types.

pub mod config {
    pub const SCAN_INTERVAL_MS: u8 = 0x01;
    pub const SAME_BARCODE_DELAY: u8 = 0x85;
    /// Command mode, silent: aim-LED while reading, no decode buzzer.
    /// Decode performance is identical to the buzzer-armed 0xD1 (rig
    /// A/B 2026-09-10, 24/24 scans, same latency) — specter-diy's 0xD1
    /// legacy adds only the beep. Hosts wanting it: SetSettings(0xD1).
    // GM65: | 6 | buzzer (decode beep) | 1 = beep on every decode |
    // GM65: | 1:0 | read mode | 00 manual, 01 command, 10 continuous, 11 induction |
    // GM65: In Command mode (01) the module ACKs ScanEnable writes but never scans.
    pub const CMD_MODE: u8 = 0x91;
    pub const VERSION_NEEDS_RAW: u8 = 0x69;
    pub const RAW_MODE_VALUE: u8 = 0x08;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AimSetting {
    Off = 0b00,
    Reading = 0b01,
    Always = 0b10,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum LightSetting {
    Off = 0b00,
    Reading = 0b01,
    Always = 0b10,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ReadMode {
    Manual = 0b00,
    Command = 0b01,
    Continuous = 0b10,
    Induction = 0b11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ScannerSettings {
    pub always_on: bool,
    pub buzzer: bool,
    pub aim: AimSetting,
    pub light: LightSetting,
    pub read_mode: ReadMode,
}

impl ScannerSettings {
    // GM65: | 7 | always_on | 1 = scanner always powered |
    // GM65: | 5:4 | aim | 00 off, 01 while reading, 1x always |
    // GM65: | 3:2 | light | 00 off, 01 while reading, 1x always |
    pub fn bits(&self) -> u8 {
        let mut val: u8 = 0;
        if self.always_on {
            val |= 1 << 7;
        }
        if self.buzzer {
            val |= 1 << 6;
        }
        val |= (self.aim as u8 & 0b11) << 4;
        val |= (self.light as u8 & 0b11) << 2;
        val |= self.read_mode as u8 & 0b11;
        val
    }

    pub fn from_bits(raw: u8) -> Self {
        Self {
            always_on: raw & (1 << 7) != 0,
            buzzer: raw & (1 << 6) != 0,
            aim: match (raw >> 4) & 0b11 {
                0b00 => AimSetting::Off,
                0b01 => AimSetting::Reading,
                _ => AimSetting::Always,
            },
            light: match (raw >> 2) & 0b11 {
                0b00 => LightSetting::Off,
                0b01 => LightSetting::Reading,
                _ => LightSetting::Always,
            },
            read_mode: match raw & 0b11 {
                0b00 => ReadMode::Manual,
                0b01 => ReadMode::Command,
                0b10 => ReadMode::Continuous,
                _ => ReadMode::Induction,
            },
        }
    }
}

impl Default for ScannerSettings {
    fn default() -> Self {
        // Silent command mode — must equal config::CMD_MODE (0x91).
        Self {
            always_on: true,
            buzzer: false,
            aim: AimSetting::Reading,
            light: LightSetting::Off,
            read_mode: ReadMode::Command,
        }
    }
}
