//! HID POS Barcode Scanner interface (Usage Page 0x8C).
//!
//! **Experimental**: This module provides report descriptor and report
//! structures per USB-IF HID POS Usage Tables 1.02. It is not yet
//! wired into any firmware binary.
//!
//! # Standards References
//!
//! - **USB-IF HID Point of Sale Usage Tables 1.02**:
//!   <https://www.usb.org/sites/default/files/pos1_02.pdf>
//!
//! - **ISO/IEC 15424**: AIM symbology identifiers used in the
//!   symbology field of POS reports.
//!
//! # Target Compatibility (unvalidated)
//!
//! The descriptor and report format in this module are designed per the
//! HID POS 1.02 spec. Once integrated into firmware, the following host
//! APIs *should* be compatible, but this has **not been validated**:
//! - Windows POS for .NET / UWP BarcodeScanner API
//! - Linux hidraw / libhid
//! - WebHID API
//!
//! # Key Usages (from HID POS Usage Tables 1.02, §3)
//!
//! - `0x8C/0x02`: Bar Code Scanner (application collection)
//! - `0x8C/0x10`: Decoded Data
//! - `0x8C/0x12`: Decoded Data Length
//! - `0x8C/0x30`: Symbology Identifier (1-3 byte AIM ID)

/// HID POS Barcode Scanner report descriptor.
///
/// Implements a minimal HID POS interface per USB-IF HID POS Usage Tables 1.02.
/// This descriptor defines:
/// - Usage Page 0x8C (Bar Code Scanner)
/// - A variable-length decoded data field (up to 256 bytes)
/// - A data length field
/// - A symbology identifier field (AIM code)
pub const POS_BARCODE_SCANNER_REPORT_DESCRIPTOR: &[u8] = &[
    0x06, 0x8C, 0x00, // Usage Page (Bar Code Scanner)     — HID POS 1.02, §3
    0x09, 0x02, //       Usage (Bar Code Scanner)           — HID POS 1.02, §3
    0xA1, 0x01, //       Collection (Application)
    // Decoded data (variable length, up to 256 bytes)
    0x09, 0x10, //         Usage (Decoded Data)             — HID POS 1.02, §3
    0x15, 0x00, //         Logical Minimum (0)
    0x26, 0xFF, 0x00, //   Logical Maximum (255)
    0x75, 0x08, //         Report Size (8)
    0x96, 0x00, 0x01, //   Report Count (256)
    0x81, 0x02, //         Input (Data, Variable, Absolute)
    // Decoded data length (2 bytes)
    0x09, 0x12, //         Usage (Decoded Data Length)      — HID POS 1.02, §3
    0x15, 0x00, //         Logical Minimum (0)
    0x26, 0xFF, 0x00, //   Logical Maximum (255)
    0x75, 0x10, //         Report Size (16)
    0x95, 0x01, //         Report Count (1)
    0x81, 0x02, //         Input (Data, Variable, Absolute)
    // Symbology identifier (AIM code, 3 bytes)
    0x09, 0x30, //         Usage (Symbology Identifier)    — HID POS 1.02, §3; ISO/IEC 15424
    0x15, 0x00, //         Logical Minimum (0)
    0x26, 0xFF, 0x00, //   Logical Maximum (255)
    0x75, 0x08, //         Report Size (8)
    0x95, 0x03, //         Report Count (3)
    0x81, 0x02, //         Input (Data, Variable, Absolute)
    0xC0, //             End Collection
];

/// HID POS Barcode Scanner report.
///
/// Per USB-IF HID POS Usage Tables 1.02.
/// Contains decoded barcode data, its length, and the AIM symbology identifier.
#[derive(Debug, Clone)]
pub struct HidPosReport {
    /// Decoded barcode data (up to 256 bytes, zero-padded).
    pub data: [u8; 256],
    /// Actual length of decoded data.
    pub data_length: u16,
    /// AIM symbology identifier (3 bytes, e.g., b"]Q3" for QR Code).
    ///
    /// Per ISO/IEC 15424 (referenced by HID POS 1.02, §3).
    pub symbology: [u8; 3],
}

impl HidPosReport {
    /// Create a new HID POS report from scan data.
    ///
    /// `symbology` should be the 3-byte AIM identifier per ISO/IEC 15424.
    /// Common values:
    /// - `b"]Q3"` — QR Code
    /// - `b"]E0"` — EAN-13
    /// - `b"]C0"` — Code 128
    /// - `b"]A0"` — Code 39
    #[must_use]
    pub fn new(scan_data: &[u8], symbology: [u8; 3]) -> Self {
        let mut data = [0u8; 256];
        let len = scan_data.len().min(256);
        data[..len].copy_from_slice(&scan_data[..len]);
        Self {
            data,
            data_length: len as u16,
            symbology,
        }
    }

    /// Serialize to the HID report byte array.
    ///
    /// Layout: 256 bytes data + 2 bytes length (LE) + 3 bytes symbology = 261 bytes.
    #[must_use]
    pub fn as_bytes(&self) -> [u8; 261] {
        let mut buf = [0u8; 261];
        buf[..256].copy_from_slice(&self.data);
        buf[256..258].copy_from_slice(&self.data_length.to_le_bytes());
        buf[258..261].copy_from_slice(&self.symbology);
        buf
    }

    /// AIM symbology identifier for QR Code (per ISO/IEC 15424).
    pub const SYMBOLOGY_QR: [u8; 3] = *b"]Q3";
    /// AIM symbology identifier for EAN-13 (per ISO/IEC 15424).
    pub const SYMBOLOGY_EAN13: [u8; 3] = *b"]E0";
    /// AIM symbology identifier for Code 128 (per ISO/IEC 15424).
    pub const SYMBOLOGY_CODE128: [u8; 3] = *b"]C0";
    /// AIM symbology identifier for Code 39 (per ISO/IEC 15424).
    pub const SYMBOLOGY_CODE39: [u8; 3] = *b"]A0";
    /// AIM symbology identifier for DataMatrix (per ISO/IEC 15424).
    pub const SYMBOLOGY_DATAMATRIX: [u8; 3] = *b"]d2";
    /// Unknown / unavailable symbology identifier.
    ///
    /// Use this when the transport does not provide a reliable AIM code.
    pub const SYMBOLOGY_UNKNOWN: [u8; 3] = [0x00, 0x00, 0x00];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pos_descriptor_usage_page() {
        assert_eq!(POS_BARCODE_SCANNER_REPORT_DESCRIPTOR[0], 0x06);
        assert_eq!(POS_BARCODE_SCANNER_REPORT_DESCRIPTOR[1], 0x8C);
        assert_eq!(POS_BARCODE_SCANNER_REPORT_DESCRIPTOR[2], 0x00);
        assert_eq!(*POS_BARCODE_SCANNER_REPORT_DESCRIPTOR.last().unwrap(), 0xC0);
    }

    #[test]
    fn test_pos_report_new() {
        let report = HidPosReport::new(b"Hello", HidPosReport::SYMBOLOGY_QR);
        assert_eq!(report.data_length, 5);
        assert_eq!(&report.data[..5], b"Hello");
        assert_eq!(&report.data[5..], &[0u8; 251]);
        assert_eq!(report.symbology, *b"]Q3");
    }

    #[test]
    fn test_pos_report_as_bytes() {
        let report = HidPosReport::new(b"AB", HidPosReport::SYMBOLOGY_EAN13);
        let bytes = report.as_bytes();
        assert_eq!(bytes.len(), 261);
        assert_eq!(bytes[0], b'A');
        assert_eq!(bytes[1], b'B');
        assert_eq!(bytes[2], 0);
        // Length at offset 256 (LE u16)
        assert_eq!(bytes[256], 2);
        assert_eq!(bytes[257], 0);
        // Symbology at offset 258
        assert_eq!(&bytes[258..261], b"]E0");
    }

    #[test]
    fn test_pos_report_max_data() {
        let data = [0xAA; 300];
        let report = HidPosReport::new(&data, HidPosReport::SYMBOLOGY_CODE128);
        assert_eq!(report.data_length, 256); // truncated
    }

    #[test]
    fn test_pos_symbology_constants() {
        assert_eq!(HidPosReport::SYMBOLOGY_QR, *b"]Q3");
        assert_eq!(HidPosReport::SYMBOLOGY_EAN13, *b"]E0");
        assert_eq!(HidPosReport::SYMBOLOGY_CODE128, *b"]C0");
        assert_eq!(HidPosReport::SYMBOLOGY_CODE39, *b"]A0");
        assert_eq!(HidPosReport::SYMBOLOGY_DATAMATRIX, *b"]d2");
        assert_eq!(HidPosReport::SYMBOLOGY_UNKNOWN, [0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_pos_report_empty_data() {
        let report = HidPosReport::new(b"", HidPosReport::SYMBOLOGY_QR);
        assert_eq!(report.data_length, 0);
        assert_eq!(report.data, [0u8; 256]);
        let bytes = report.as_bytes();
        assert_eq!(bytes[256], 0); // length low
        assert_eq!(bytes[257], 0); // length high
    }

    #[test]
    fn test_pos_report_exactly_256_bytes() {
        // Boundary: exactly max capacity
        let data = [0x42; 256];
        let report = HidPosReport::new(&data, HidPosReport::SYMBOLOGY_CODE128);
        assert_eq!(report.data_length, 256);
        assert_eq!(report.data, [0x42; 256]);
        let bytes = report.as_bytes();
        // 256 as LE u16 = [0x00, 0x01]
        assert_eq!(bytes[256], 0x00);
        assert_eq!(bytes[257], 0x01);
    }

    #[test]
    fn test_pos_report_truncation_at_257() {
        // One byte over max — should truncate to 256
        let data = [0xBB; 257];
        let report = HidPosReport::new(&data, HidPosReport::SYMBOLOGY_QR);
        assert_eq!(report.data_length, 256);
    }

    #[test]
    fn test_pos_descriptor_length() {
        // Sanity check: descriptor should be a reasonable size
        assert!(POS_BARCODE_SCANNER_REPORT_DESCRIPTOR.len() > 10);
        assert!(POS_BARCODE_SCANNER_REPORT_DESCRIPTOR.len() < 200);
    }

    #[test]
    fn test_pos_report_as_bytes_total_length() {
        // Report must always be exactly 261 bytes: 256 data + 2 length + 3 symbology
        let report = HidPosReport::new(b"test", HidPosReport::SYMBOLOGY_QR);
        assert_eq!(report.as_bytes().len(), 261);
    }
}
