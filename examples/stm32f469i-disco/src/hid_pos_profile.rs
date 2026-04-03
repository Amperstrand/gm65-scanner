use gm65_scanner::hid::pos::HidPosReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosSymbologySource {
    TransportAim,
    UnknownTransportUnavailable,
}

#[derive(Debug, Clone)]
pub struct BuiltPosReport {
    pub report: HidPosReport,
    pub was_truncated: bool,
    pub symbology_source: PosSymbologySource,
}

pub const fn resolve_symbology(transport_aim: Option<[u8; 3]>) -> ([u8; 3], PosSymbologySource) {
    match transport_aim {
        Some(aim) => (aim, PosSymbologySource::TransportAim),
        None => (
            HidPosReport::SYMBOLOGY_UNKNOWN,
            PosSymbologySource::UnknownTransportUnavailable,
        ),
    }
}

pub fn build_pos_report(scan_data: &[u8], transport_aim: Option<[u8; 3]>) -> BuiltPosReport {
    let (symbology, symbology_source) = resolve_symbology(transport_aim);
    BuiltPosReport {
        report: HidPosReport::new(scan_data, symbology),
        was_truncated: scan_data.len() > 256,
        symbology_source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_transport_aim_when_available() {
        let built = build_pos_report(b"abc", Some(HidPosReport::SYMBOLOGY_QR));

        assert_eq!(built.symbology_source, PosSymbologySource::TransportAim);
        assert_eq!(built.report.symbology, HidPosReport::SYMBOLOGY_QR);
        assert!(!built.was_truncated);
    }

    #[test]
    fn falls_back_to_unknown_when_aim_unavailable() {
        let built = build_pos_report(b"abc", None);

        assert_eq!(
            built.symbology_source,
            PosSymbologySource::UnknownTransportUnavailable
        );
        assert_eq!(built.report.symbology, HidPosReport::SYMBOLOGY_UNKNOWN);
    }

    #[test]
    fn truncation_boundary_is_explicit() {
        let exact = build_pos_report(&[0x55; 256], None);
        let over = build_pos_report(&[0xAA; 257], None);

        assert!(!exact.was_truncated);
        assert_eq!(exact.report.data_length, 256);
        assert!(over.was_truncated);
        assert_eq!(over.report.data_length, 256);
    }
}
