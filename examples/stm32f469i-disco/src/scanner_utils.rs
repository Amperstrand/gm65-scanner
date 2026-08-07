use gm65_scanner::{
    PayloadType, ScannerModel, ScannerSettings, AimSetting, LightSetting, ReadMode,
};

pub fn model_to_str(model: ScannerModel) -> &'static str {
    match model {
        ScannerModel::Gm65 => "GM65",
        ScannerModel::M3Y => "M3Y",
        ScannerModel::Generic => "Generic",
        ScannerModel::Unknown => "Unknown",
    }
}

pub fn payload_type_to_byte(pt: PayloadType) -> u8 {
    match pt {
        PayloadType::CashuV4 => 0x01,
        PayloadType::CashuV3 => 0x02,
        PayloadType::UrFragment => 0x03,
        PayloadType::PlainText | PayloadType::Url => 0x00,
        PayloadType::Binary => 0x04,
    }
}

pub fn model_to_status_byte(model: ScannerModel) -> u8 {
    match model {
        ScannerModel::Gm65 => 0x01,
        ScannerModel::M3Y => 0x02,
        ScannerModel::Generic => 0x03,
        ScannerModel::Unknown => 0x00,
    }
}

/// Toggle the scanner setting for the given settings row (0-indexed).
/// Row 0: buzzer, Row 1: aim, Row 2: light, Row 3: continuous mode.
/// Returns true if the row was valid and toggled.
pub fn toggle_settings_row(settings: &mut ScannerSettings, row: usize) -> bool {
    match row {
        0 => {
            settings.buzzer = !settings.buzzer;
            true
        }
        1 => {
            settings.aim = match settings.aim {
                AimSetting::Off | AimSetting::Always => AimSetting::Reading,
                AimSetting::Reading => AimSetting::Off,
            };
            true
        }
        2 => {
            settings.light = match settings.light {
                LightSetting::Off | LightSetting::Always => LightSetting::Reading,
                LightSetting::Reading => LightSetting::Off,
            };
            true
        }
        3 => {
            settings.read_mode = match settings.read_mode {
                ReadMode::Command => ReadMode::Continuous,
                _ => ReadMode::Command,
            };
            true
        }
        _ => false,
    }
}

pub fn build_scanner_status_payload(connected: bool, initialized: bool, model_byte: u8) -> [u8; 3] {
    [connected as u8, initialized as u8, model_byte]
}
