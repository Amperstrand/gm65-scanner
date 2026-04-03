use gm65_scanner::ScannerSettings;

pub const PROFILE_FLASH_MAGIC: u32 = 0x4453_3232; // "DS22"
pub const PROFILE_FLASH_VERSION: u8 = 1;
pub const PROFILE_FLASH_BYTES: usize = 64;
pub const PROFILE_PREFIX_MAX: usize = 8;
pub const PROFILE_SUFFIX_MAX: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UsbMode {
    Ds2208KeyboardHid = 0x01,
    Ds2208HidPos = 0x02,
    AdminCdc = 0x03,
}

impl UsbMode {
    pub const fn cycle(self) -> Self {
        match self {
            Self::Ds2208KeyboardHid => Self::Ds2208HidPos,
            Self::Ds2208HidPos => Self::AdminCdc,
            Self::AdminCdc => Self::Ds2208KeyboardHid,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Ds2208KeyboardHid => "DS2208 Keyboard HID",
            Self::Ds2208HidPos => "DS2208 HID POS",
            Self::AdminCdc => "Admin CDC",
        }
    }
}

impl TryFrom<u8> for UsbMode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::Ds2208KeyboardHid),
            0x02 => Ok(Self::Ds2208HidPos),
            0x03 => Ok(Self::AdminCdc),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SuffixMode {
    None = 0x00,
    Enter = 0x01,
    Tab = 0x02,
}

impl SuffixMode {
    pub const fn cycle(self) -> Self {
        match self {
            Self::None => Self::Enter,
            Self::Enter => Self::Tab,
            Self::Tab => Self::None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Enter => "Enter",
            Self::Tab => "Tab",
        }
    }
}

impl TryFrom<u8> for SuffixMode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::None),
            0x01 => Ok(Self::Enter),
            0x02 => Ok(Self::Tab),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CaseMode {
    Preserve = 0x00,
    Upper = 0x01,
    Lower = 0x02,
}

impl CaseMode {
    pub const fn cycle(self) -> Self {
        match self {
            Self::Preserve => Self::Upper,
            Self::Upper => Self::Lower,
            Self::Lower => Self::Preserve,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Preserve => "Preserve",
            Self::Upper => "Upper",
            Self::Lower => "Lower",
        }
    }
}

impl TryFrom<u8> for CaseMode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Preserve),
            0x01 => Ok(Self::Upper),
            0x02 => Ok(Self::Lower),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatibilityProfile {
    pub usb_mode: UsbMode,
    pub suffix: SuffixMode,
    pub key_delay_ms: u8,
    pub case_mode: CaseMode,
    pub fast_hid: bool,
    pub caps_lock_override: bool,
    pub simulated_caps_lock: bool,
    pub scanner_settings: u8,
    pub prefix_len: u8,
    pub suffix_bytes_len: u8,
    pub prefix: [u8; PROFILE_PREFIX_MAX],
    pub suffix_bytes: [u8; PROFILE_SUFFIX_MAX],
}

impl Default for CompatibilityProfile {
    fn default() -> Self {
        Self {
            usb_mode: UsbMode::Ds2208KeyboardHid,
            suffix: SuffixMode::Enter,
            key_delay_ms: 0,
            case_mode: CaseMode::Preserve,
            fast_hid: true,
            caps_lock_override: true,
            simulated_caps_lock: false,
            scanner_settings: (ScannerSettings::ALWAYS_ON
                | ScannerSettings::COMMAND
                | ScannerSettings::SOUND)
                .bits(),
            prefix_len: 0,
            suffix_bytes_len: 0,
            prefix: [0; PROFILE_PREFIX_MAX],
            suffix_bytes: [0; PROFILE_SUFFIX_MAX],
        }
    }
}

impl CompatibilityProfile {
    pub const fn key_delay_label(self) -> &'static str {
        match self.key_delay_ms {
            0 => "0 ms",
            20 => "20 ms",
            40 => "40 ms",
            _ => "custom",
        }
    }

    pub fn cycle_key_delay(&mut self) {
        self.key_delay_ms = match self.key_delay_ms {
            0 => 20,
            20 => 40,
            _ => 0,
        };
    }

    pub fn transform_ascii(&self, byte: u8) -> u8 {
        match self.case_mode {
            CaseMode::Preserve => byte,
            CaseMode::Upper => byte.to_ascii_uppercase(),
            CaseMode::Lower => byte.to_ascii_lowercase(),
        }
    }

    pub const fn needs_reenumeration_to(self, next: Self) -> bool {
        self.usb_mode as u8 != next.usb_mode as u8 || self.fast_hid != next.fast_hid
    }

    pub fn serialize(self) -> [u8; PROFILE_FLASH_BYTES] {
        let mut out = [0xFFu8; PROFILE_FLASH_BYTES];
        out[0..4].copy_from_slice(&PROFILE_FLASH_MAGIC.to_le_bytes());
        out[4] = PROFILE_FLASH_VERSION;
        out[5] = self.usb_mode as u8;
        out[6] = self.suffix as u8;
        out[7] = self.key_delay_ms;
        out[8] = self.case_mode as u8;
        out[9] = u8::from(self.fast_hid);
        out[10] = u8::from(self.caps_lock_override);
        out[11] = u8::from(self.simulated_caps_lock);
        out[12] = self.scanner_settings;
        out[13] = self.prefix_len.min(PROFILE_PREFIX_MAX as u8);
        out[14] = self.suffix_bytes_len.min(PROFILE_SUFFIX_MAX as u8);
        out[16..24].copy_from_slice(&self.prefix);
        out[24..32].copy_from_slice(&self.suffix_bytes);

        let checksum = checksum32(&out[..60]);
        out[60..64].copy_from_slice(&checksum.to_le_bytes());
        out
    }

    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < PROFILE_FLASH_BYTES {
            return None;
        }
        let magic = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
        if magic != PROFILE_FLASH_MAGIC || bytes[4] != PROFILE_FLASH_VERSION {
            return None;
        }
        let expected = checksum32(&bytes[..60]);
        let actual = u32::from_le_bytes(bytes[60..64].try_into().ok()?);
        if expected != actual {
            return None;
        }

        let prefix_len = bytes[13].min(PROFILE_PREFIX_MAX as u8);
        let suffix_bytes_len = bytes[14].min(PROFILE_SUFFIX_MAX as u8);
        let mut prefix = [0u8; PROFILE_PREFIX_MAX];
        let mut suffix_bytes = [0u8; PROFILE_SUFFIX_MAX];
        prefix.copy_from_slice(&bytes[16..24]);
        suffix_bytes.copy_from_slice(&bytes[24..32]);

        Some(Self {
            usb_mode: UsbMode::try_from(bytes[5]).ok()?,
            suffix: SuffixMode::try_from(bytes[6]).ok()?,
            key_delay_ms: bytes[7],
            case_mode: CaseMode::try_from(bytes[8]).ok()?,
            fast_hid: bytes[9] != 0,
            caps_lock_override: bytes[10] != 0,
            simulated_caps_lock: bytes[11] != 0,
            scanner_settings: bytes[12],
            prefix_len,
            suffix_bytes_len,
            prefix,
            suffix_bytes,
        })
    }

    pub fn set_prefix(&mut self, bytes: &[u8]) {
        self.prefix.fill(0);
        let len = bytes.len().min(PROFILE_PREFIX_MAX);
        self.prefix[..len].copy_from_slice(&bytes[..len]);
        self.prefix_len = len as u8;
    }

    pub fn set_suffix_bytes(&mut self, bytes: &[u8]) {
        self.suffix_bytes.fill(0);
        let len = bytes.len().min(PROFILE_SUFFIX_MAX);
        self.suffix_bytes[..len].copy_from_slice(&bytes[..len]);
        self.suffix_bytes_len = len as u8;
    }

    pub fn prefix_slice(&self) -> &[u8] {
        &self.prefix[..self.prefix_len as usize]
    }

    pub fn suffix_bytes_slice(&self) -> &[u8] {
        &self.suffix_bytes[..self.suffix_bytes_len as usize]
    }
}

pub fn checksum32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |acc, &b| {
        acc.wrapping_mul(16777619).wrapping_add(u32::from(b))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_is_ds2208_keyboard() {
        let profile = CompatibilityProfile::default();
        assert_eq!(profile.usb_mode, UsbMode::Ds2208KeyboardHid);
        assert_eq!(profile.suffix, SuffixMode::Enter);
        assert_eq!(profile.key_delay_ms, 0);
        assert!(profile.fast_hid);
        assert!(profile.caps_lock_override);
    }

    #[test]
    fn profile_roundtrip() {
        let mut profile = CompatibilityProfile {
            usb_mode: UsbMode::AdminCdc,
            suffix: SuffixMode::Tab,
            key_delay_ms: 40,
            case_mode: CaseMode::Upper,
            fast_hid: false,
            caps_lock_override: false,
            simulated_caps_lock: true,
            ..CompatibilityProfile::default()
        };
        profile.set_prefix(b"]C1");
        profile.set_suffix_bytes(b"\r\n");

        let bytes = profile.serialize();
        assert_eq!(CompatibilityProfile::deserialize(&bytes), Some(profile));
    }

    #[test]
    fn bad_checksum_is_rejected() {
        let mut bytes = CompatibilityProfile::default().serialize();
        bytes[20] ^= 0x55;
        assert_eq!(CompatibilityProfile::deserialize(&bytes), None);
    }

    #[test]
    fn cycles_are_stable() {
        assert_eq!(
            UsbMode::Ds2208KeyboardHid.cycle().cycle().cycle(),
            UsbMode::Ds2208KeyboardHid
        );
        assert_eq!(SuffixMode::None.cycle().cycle().cycle(), SuffixMode::None);
        assert_eq!(
            CaseMode::Preserve.cycle().cycle().cycle(),
            CaseMode::Preserve
        );
    }
}
