//! GM65 scanner configuration constants and settings types.

pub mod config {
    pub const SCAN_INTERVAL_MS: u8 = 0x01;
    pub const SAME_BARCODE_DELAY: u8 = 0x85;
    pub const CMD_MODE: u8 = 0xD1;
    pub const VERSION_NEEDS_RAW: u8 = 0x69;
    pub const RAW_MODE_VALUE: u8 = 0x08;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AimSetting {
    Off     = 0b00,
    Reading = 0b01,
    Always  = 0b10,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum LightSetting {
    Off     = 0b00,
    Reading = 0b01,
    Always  = 0b10,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ReadMode {
    Manual     = 0b00,
    Command    = 0b01,
    Continuous = 0b10,
    Induction  = 0b11,
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
    pub fn bits(&self) -> u8 {
        let mut val: u8 = 0;
        if self.always_on { val |= 1 << 7; }
        if self.buzzer    { val |= 1 << 6; }
        val |= (self.aim as u8 & 0b11) << 4;
        val |= (self.light as u8 & 0b11) << 2;
        val |= self.read_mode as u8 & 0b11;
        val
    }

    pub fn from_bits(raw: u8) -> Self {
        Self {
            always_on: raw & (1 << 7) != 0,
            buzzer:    raw & (1 << 6) != 0,
            aim:       match (raw >> 4) & 0b11 {
                0b00 => AimSetting::Off,
                0b01 => AimSetting::Reading,
                _    => AimSetting::Always,
            },
            light:     match (raw >> 2) & 0b11 {
                0b00 => LightSetting::Off,
                0b01 => LightSetting::Reading,
                _    => LightSetting::Always,
            },
            read_mode: match raw & 0b11 {
                0b00 => ReadMode::Manual,
                0b01 => ReadMode::Command,
                0b10 => ReadMode::Continuous,
                _    => ReadMode::Induction,
            },
        }
    }
}

impl Default for ScannerSettings {
    fn default() -> Self {
        Self {
            always_on: true,
            buzzer:    true,
            aim:       AimSetting::Reading,
            light:     LightSetting::Off,
            read_mode: ReadMode::Command,
        }
    }
}
