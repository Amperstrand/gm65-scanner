use gm65_scanner::{PayloadType, ScannerModel, ScannerSettings};

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

pub fn row_to_settings_flag(row: usize) -> Option<ScannerSettings> {
    match row {
        0 => Some(ScannerSettings::SOUND),
        1 => Some(ScannerSettings::AIM),
        2 => Some(ScannerSettings::LIGHT),
        3 => Some(ScannerSettings::CONTINUOUS),
        4 => Some(ScannerSettings::COMMAND),
        _ => None,
    }
}

pub fn build_scanner_status_payload(connected: bool, initialized: bool, model_byte: u8) -> [u8; 3] {
    [connected as u8, initialized as u8, model_byte]
}
