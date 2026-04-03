//! HID Keyboard Wedge — barcode-to-keystroke mapping.
//!
//! Converts scanned barcode data into USB HID keyboard reports,
//! enabling the scanner to act as a "keyboard wedge" that types
//! barcode data into any focused application.
//!
//! # Standards References
//!
//! - **USB HID Usage Tables 1.5, §10 "Keyboard/Keypad Page" (Usage Page 0x07)**:
//!   Defines the key codes used in HID keyboard reports.
//!   <https://usb.org/sites/default/files/hut1_5.pdf>
//!
//! - **USB HID 1.11, Appendix B "Boot Interface Descriptors"**:
//!   Defines the 8-byte boot keyboard report format used here.
//!   <https://www.usb.org/sites/default/files/hid1_11.pdf>
//!
//! # Keyboard Wedge Mode
//!
//! Commercial barcode scanners (Zebra, Honeywell, Datalogic) commonly
//! support "HID Keyboard Wedge" mode where scanned data is output as
//! simulated keystrokes. This module implements the same pattern:
//!
//! 1. Each byte of barcode data is mapped to a HID key code + modifier
//! 2. A press report (key down) is sent, followed by a release report (all keys up)
//! 3. A configurable terminator key (Enter/Tab/None) is sent after the data
//!
//! # Unmappable Byte Policy
//!
//! Bytes outside printable ASCII (0x20–0x7E) are **silently skipped**.
//! This includes control characters (0x00–0x1F), DEL (0x7F), and all
//! bytes > 0x7F. The `is_mappable()` method can be used to check
//! individual bytes before mapping.
//!
//! # Layout
//!
//! Currently supports US English (QWERTY) layout. The mapping covers
//! printable ASCII (0x20–0x7E).
//!
//! # Example
//!
//! ```rust
//! use gm65_scanner::hid::keyboard::{HidKeyboardReport, KeyMapper, Terminator, US_ENGLISH};
//!
//! let mapper = KeyMapper::new(&US_ENGLISH, Terminator::Enter);
//! let data = b"Hello123";
//! let mut reports = mapper.map_to_reports(data);
//!
//! // Each character produces a press + release report pair
//! let press = reports.next().unwrap(); // 'H' key down (with Shift)
//! assert!(press.modifier & 0x02 != 0); // Left Shift
//! let release = reports.next().unwrap(); // key up
//! assert_eq!(release.keycode, 0x00); // no key
//! ```

/// USB HID Boot Keyboard Report (8 bytes).
///
/// Per USB HID 1.11, Appendix B.1 "Protocol 1 (Keyboard)":
/// - Byte 0: Modifier keys (bitmask)
/// - Byte 1: Reserved (0x00)
/// - Bytes 2–7: Key codes (up to 6 simultaneous keys)
///
/// For keyboard wedge output we only need one key at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HidKeyboardReport {
    /// Modifier key bitmask.
    ///
    /// Per USB HID Usage Tables 1.5, §10, Table 12:
    /// - Bit 0: Left Control
    /// - Bit 1: Left Shift
    /// - Bit 2: Left Alt
    /// - Bit 3: Left GUI
    /// - Bit 4: Right Control
    /// - Bit 5: Right Shift
    /// - Bit 6: Right Alt
    /// - Bit 7: Right GUI
    pub modifier: u8,

    /// Reserved byte, always 0x00 per spec.
    pub reserved: u8,

    /// Primary key code (Usage Page 0x07).
    ///
    /// Key codes per USB HID Usage Tables 1.5, §10.
    /// 0x00 = no key (release report).
    pub keycode: u8,
}

impl HidKeyboardReport {
    /// Serialize to the 8-byte boot keyboard report format.
    ///
    /// Per USB HID 1.11, Appendix B.1.
    #[must_use]
    pub fn as_bytes(&self) -> [u8; 8] {
        [
            self.modifier,
            self.reserved,
            self.keycode,
            0x00, // keys 2-6 unused
            0x00,
            0x00,
            0x00,
            0x00,
        ]
    }

    /// Create a "no keys pressed" release report.
    #[must_use]
    pub const fn release() -> Self {
        Self {
            modifier: 0,
            reserved: 0,
            keycode: 0,
        }
    }

    /// Create a key press report.
    #[must_use]
    pub const fn press(modifier: u8, keycode: u8) -> Self {
        Self {
            modifier,
            reserved: 0,
            keycode,
        }
    }
}

// ============================================================================
// Modifier key constants
// Per USB HID Usage Tables 1.5, §10, Table 12 "Keyboard/Keypad Page"
// ============================================================================

/// Left Shift modifier bit (USB HID Usage Tables 1.5, §10).
pub const MOD_LEFT_SHIFT: u8 = 0x02;

// ============================================================================
// Key code constants — USB HID Usage Tables 1.5, §10 "Keyboard/Keypad Page"
// Usage Page 0x07
// ============================================================================

/// Key code: Enter/Return (Usage ID 0x28).
pub const KEY_ENTER: u8 = 0x28;
/// Key code: Tab (Usage ID 0x2B).
pub const KEY_TAB: u8 = 0x2B;

/// Terminator key sent after barcode data.
///
/// Commercial scanners (Zebra, Honeywell) typically default to Enter.
/// POS software often expects Enter to submit the scanned value.
/// Tab is used when scanning into multi-field forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminator {
    /// Send Enter (0x28) after barcode data (most common POS default).
    Enter,
    /// Send Tab (0x2B) after barcode data (for multi-field forms).
    Tab,
    /// No terminator key.
    None,
}

impl Terminator {
    /// Get the HID key code for this terminator, if any.
    #[must_use]
    pub const fn keycode(&self) -> Option<u8> {
        match self {
            Terminator::Enter => Some(KEY_ENTER),
            Terminator::Tab => Some(KEY_TAB),
            Terminator::None => Option::None,
        }
    }
}

/// ASCII-to-HID key code mapping entry.
///
/// Maps a single ASCII character to its HID key code and whether
/// the Shift modifier is required.
#[derive(Debug, Clone, Copy)]
pub struct KeyMapping {
    /// HID key code from Usage Page 0x07 (0 = unmapped).
    pub keycode: u8,
    /// Whether Left Shift modifier is required for this character.
    pub shifted: bool,
}

impl KeyMapping {
    /// Create a new key mapping.
    #[must_use]
    pub const fn new(keycode: u8, shifted: bool) -> Self {
        Self { keycode, shifted }
    }

    /// Unmapped character sentinel.
    pub const NONE: Self = Self {
        keycode: 0,
        shifted: false,
    };
}

/// Keyboard layout: 128-entry table mapping ASCII to HID key codes.
///
/// Index by ASCII value (0x00–0x7F). Entries with keycode == 0 are unmapped.
///
/// # US English (QWERTY) Layout
///
/// The US_ENGLISH constant provides the standard mapping per
/// USB HID Usage Tables 1.5, §10 and the physical US QWERTY layout.
pub type KeyboardLayout = [KeyMapping; 128];

// ============================================================================
// US English QWERTY layout
//
// Key code assignments per USB HID Usage Tables 1.5, §10 "Keyboard/Keypad
// Page" (Usage Page 0x07). Mapping from ASCII values to HID key codes with
// shift state derived from the standard US QWERTY keyboard layout.
//
// Reference implementations:
// - MightyPork/usb_hid_keys.h (community reference)
//   https://gist.github.com/MightyPork/6da26e382a7ad91b5496ee55fdc73db2
// - Fabi019/hid-barcode-scanner (Android BLE HID keyboard wedge)
//   https://github.com/Fabi019/hid-barcode-scanner
// - dlkj/usbd-human-interface-device (Rust embedded HID)
//   https://github.com/dlkj/usbd-human-interface-device
// ============================================================================

/// US English QWERTY keyboard layout.
///
/// Covers printable ASCII (0x20–0x7E). Key codes per USB HID Usage Tables 1.5.
pub const US_ENGLISH: KeyboardLayout = {
    let mut layout = [KeyMapping::NONE; 128];

    // Space (0x20) → Key code 0x2C
    layout[0x20] = KeyMapping::new(0x2C, false); // ' '

    // Shifted symbols (0x21–0x29)
    layout[0x21] = KeyMapping::new(0x1E, true); // '!' = Shift+1
    layout[0x22] = KeyMapping::new(0x34, true); // '"' = Shift+'
    layout[0x23] = KeyMapping::new(0x20, true); // '#' = Shift+3
    layout[0x24] = KeyMapping::new(0x21, true); // '$' = Shift+4
    layout[0x25] = KeyMapping::new(0x22, true); // '%' = Shift+5
    layout[0x26] = KeyMapping::new(0x24, true); // '&' = Shift+7
    layout[0x27] = KeyMapping::new(0x34, false); // '\''
    layout[0x28] = KeyMapping::new(0x26, true); // '(' = Shift+9
    layout[0x29] = KeyMapping::new(0x27, true); // ')' = Shift+0

    // More symbols (0x2A–0x2F)
    layout[0x2A] = KeyMapping::new(0x25, true); // '*' = Shift+8
    layout[0x2B] = KeyMapping::new(0x2E, true); // '+' = Shift+=
    layout[0x2C] = KeyMapping::new(0x36, false); // ','
    layout[0x2D] = KeyMapping::new(0x2D, false); // '-'
    layout[0x2E] = KeyMapping::new(0x37, false); // '.'
    layout[0x2F] = KeyMapping::new(0x38, false); // '/'

    // Digits 0–9 (0x30–0x39)
    // Per USB HID Usage Tables 1.5, §10:
    //   '1' = 0x1E, '2' = 0x1F, ..., '9' = 0x26, '0' = 0x27
    layout[0x30] = KeyMapping::new(0x27, false); // '0'
    layout[0x31] = KeyMapping::new(0x1E, false); // '1'
    layout[0x32] = KeyMapping::new(0x1F, false); // '2'
    layout[0x33] = KeyMapping::new(0x20, false); // '3'
    layout[0x34] = KeyMapping::new(0x21, false); // '4'
    layout[0x35] = KeyMapping::new(0x22, false); // '5'
    layout[0x36] = KeyMapping::new(0x23, false); // '6'
    layout[0x37] = KeyMapping::new(0x24, false); // '7'
    layout[0x38] = KeyMapping::new(0x25, false); // '8'
    layout[0x39] = KeyMapping::new(0x26, false); // '9'

    // Colon, semicolon, etc. (0x3A–0x40)
    layout[0x3A] = KeyMapping::new(0x33, true); // ':' = Shift+;
    layout[0x3B] = KeyMapping::new(0x33, false); // ';'
    layout[0x3C] = KeyMapping::new(0x36, true); // '<' = Shift+,
    layout[0x3D] = KeyMapping::new(0x2E, false); // '='
    layout[0x3E] = KeyMapping::new(0x37, true); // '>' = Shift+.
    layout[0x3F] = KeyMapping::new(0x38, true); // '?' = Shift+/
    layout[0x40] = KeyMapping::new(0x1F, true); // '@' = Shift+2

    // Uppercase letters A–Z (0x41–0x5A)
    // Per USB HID Usage Tables 1.5, §10:
    //   'A' = 0x04, 'B' = 0x05, ..., 'Z' = 0x1D
    // Uppercase requires Left Shift modifier.
    layout[0x41] = KeyMapping::new(0x04, true); // 'A'
    layout[0x42] = KeyMapping::new(0x05, true); // 'B'
    layout[0x43] = KeyMapping::new(0x06, true); // 'C'
    layout[0x44] = KeyMapping::new(0x07, true); // 'D'
    layout[0x45] = KeyMapping::new(0x08, true); // 'E'
    layout[0x46] = KeyMapping::new(0x09, true); // 'F'
    layout[0x47] = KeyMapping::new(0x0A, true); // 'G'
    layout[0x48] = KeyMapping::new(0x0B, true); // 'H'
    layout[0x49] = KeyMapping::new(0x0C, true); // 'I'
    layout[0x4A] = KeyMapping::new(0x0D, true); // 'J'
    layout[0x4B] = KeyMapping::new(0x0E, true); // 'K'
    layout[0x4C] = KeyMapping::new(0x0F, true); // 'L'
    layout[0x4D] = KeyMapping::new(0x10, true); // 'M'
    layout[0x4E] = KeyMapping::new(0x11, true); // 'N'
    layout[0x4F] = KeyMapping::new(0x12, true); // 'O'
    layout[0x50] = KeyMapping::new(0x13, true); // 'P'
    layout[0x51] = KeyMapping::new(0x14, true); // 'Q'
    layout[0x52] = KeyMapping::new(0x15, true); // 'R'
    layout[0x53] = KeyMapping::new(0x16, true); // 'S'
    layout[0x54] = KeyMapping::new(0x17, true); // 'T'
    layout[0x55] = KeyMapping::new(0x18, true); // 'U'
    layout[0x56] = KeyMapping::new(0x19, true); // 'V'
    layout[0x57] = KeyMapping::new(0x1A, true); // 'W'
    layout[0x58] = KeyMapping::new(0x1B, true); // 'X'
    layout[0x59] = KeyMapping::new(0x1C, true); // 'Y'
    layout[0x5A] = KeyMapping::new(0x1D, true); // 'Z'

    // Brackets and backslash (0x5B–0x60)
    layout[0x5B] = KeyMapping::new(0x2F, false); // '['
    layout[0x5C] = KeyMapping::new(0x31, false); // '\\'
    layout[0x5D] = KeyMapping::new(0x30, false); // ']'
    layout[0x5E] = KeyMapping::new(0x23, true); // '^' = Shift+6
    layout[0x5F] = KeyMapping::new(0x2D, true); // '_' = Shift+-
    layout[0x60] = KeyMapping::new(0x35, false); // '`'

    // Lowercase letters a–z (0x61–0x7A)
    // Same key codes as uppercase, without Shift.
    layout[0x61] = KeyMapping::new(0x04, false); // 'a'
    layout[0x62] = KeyMapping::new(0x05, false); // 'b'
    layout[0x63] = KeyMapping::new(0x06, false); // 'c'
    layout[0x64] = KeyMapping::new(0x07, false); // 'd'
    layout[0x65] = KeyMapping::new(0x08, false); // 'e'
    layout[0x66] = KeyMapping::new(0x09, false); // 'f'
    layout[0x67] = KeyMapping::new(0x0A, false); // 'g'
    layout[0x68] = KeyMapping::new(0x0B, false); // 'h'
    layout[0x69] = KeyMapping::new(0x0C, false); // 'i'
    layout[0x6A] = KeyMapping::new(0x0D, false); // 'j'
    layout[0x6B] = KeyMapping::new(0x0E, false); // 'k'
    layout[0x6C] = KeyMapping::new(0x0F, false); // 'l'
    layout[0x6D] = KeyMapping::new(0x10, false); // 'm'
    layout[0x6E] = KeyMapping::new(0x11, false); // 'n'
    layout[0x6F] = KeyMapping::new(0x12, false); // 'o'
    layout[0x70] = KeyMapping::new(0x13, false); // 'p'
    layout[0x71] = KeyMapping::new(0x14, false); // 'q'
    layout[0x72] = KeyMapping::new(0x15, false); // 'r'
    layout[0x73] = KeyMapping::new(0x16, false); // 's'
    layout[0x74] = KeyMapping::new(0x17, false); // 't'
    layout[0x75] = KeyMapping::new(0x18, false); // 'u'
    layout[0x76] = KeyMapping::new(0x19, false); // 'v'
    layout[0x77] = KeyMapping::new(0x1A, false); // 'w'
    layout[0x78] = KeyMapping::new(0x1B, false); // 'x'
    layout[0x79] = KeyMapping::new(0x1C, false); // 'y'
    layout[0x7A] = KeyMapping::new(0x1D, false); // 'z'

    // Braces, pipe, tilde (0x7B–0x7E)
    layout[0x7B] = KeyMapping::new(0x2F, true); // '{' = Shift+[
    layout[0x7C] = KeyMapping::new(0x31, true); // '|' = Shift+backslash
    layout[0x7D] = KeyMapping::new(0x30, true); // '}' = Shift+]
    layout[0x7E] = KeyMapping::new(0x35, true); // '~' = Shift+`

    layout
};

/// HID Boot Keyboard report descriptor.
///
/// Per USB HID 1.11, Appendix B.1 and E.6. This descriptor defines an
/// 8-byte input report (modifier + reserved + 6 keycodes) and a 1-byte
/// output report (LED indicators).
///
/// This is the standard boot keyboard descriptor used by virtually all
/// USB keyboards and keyboard-wedge barcode scanners.
pub const BOOT_KEYBOARD_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, //   Usage Page (Generic Desktop)       — HID Usage Tables 1.5, §4
    0x09, 0x06, //   Usage (Keyboard)                   — HID Usage Tables 1.5, §4
    0xA1, 0x01, //   Collection (Application)
    // Modifier keys (8 bits)
    0x05, 0x07, //     Usage Page (Keyboard/Keypad)      — HID Usage Tables 1.5, §10
    0x19, 0xE0, //     Usage Minimum (Left Control)
    0x29, 0xE7, //     Usage Maximum (Right GUI)
    0x15, 0x00, //     Logical Minimum (0)
    0x25, 0x01, //     Logical Maximum (1)
    0x75, 0x01, //     Report Size (1)
    0x95, 0x08, //     Report Count (8)
    0x81, 0x02, //     Input (Data, Variable, Absolute)
    // Reserved byte
    0x95, 0x01, //     Report Count (1)
    0x75, 0x08, //     Report Size (8)
    0x81, 0x01, //     Input (Constant)
    // LED output report (5 bits + 3 padding)
    0x95, 0x05, //     Report Count (5)
    0x75, 0x01, //     Report Size (1)
    0x05, 0x08, //     Usage Page (LEDs)                 — HID Usage Tables 1.5, §11
    0x19, 0x01, //     Usage Minimum (Num Lock)
    0x29, 0x05, //     Usage Maximum (Kana)
    0x91, 0x02, //     Output (Data, Variable, Absolute)
    0x95, 0x01, //     Report Count (1)
    0x75, 0x03, //     Report Size (3)
    0x91, 0x01, //     Output (Constant) — padding
    // Key codes (6 bytes)
    0x95, 0x06, //     Report Count (6)
    0x75, 0x08, //     Report Size (8)
    0x15, 0x00, //     Logical Minimum (0)
    0x25, 0x65, //     Logical Maximum (101)
    0x05, 0x07, //     Usage Page (Keyboard/Keypad)
    0x19, 0x00, //     Usage Minimum (0)
    0x29, 0x65, //     Usage Maximum (101)
    0x81, 0x00, //     Input (Data, Array)
    0xC0, //         End Collection
];

/// Key mapper that converts byte sequences to HID keyboard reports.
///
/// Implements the keyboard wedge pattern used by commercial barcode
/// scanners: each character is sent as a press + release report pair,
/// with an optional terminator key (Enter/Tab) at the end.
pub struct KeyMapper<'a> {
    layout: &'a KeyboardLayout,
    terminator: Terminator,
}

impl<'a> KeyMapper<'a> {
    /// Create a new key mapper with the given layout and terminator.
    #[must_use]
    pub const fn new(layout: &'a KeyboardLayout, terminator: Terminator) -> Self {
        Self { layout, terminator }
    }

    /// Check if a byte can be mapped to a HID key code.
    ///
    /// Returns `true` for printable ASCII (0x20–0x7E) that has a
    /// non-zero key code in the layout table.
    #[must_use]
    pub fn is_mappable(&self, byte: u8) -> bool {
        byte <= 0x7F && self.layout[byte as usize].keycode != 0
    }

    /// Map a single byte to a key press report, if mappable.
    ///
    /// Returns `None` for bytes outside printable ASCII (0x20–0x7E)
    /// or control characters.
    #[must_use]
    pub fn map_byte(&self, byte: u8) -> Option<HidKeyboardReport> {
        if !self.is_mappable(byte) {
            return None;
        }
        let mapping = &self.layout[byte as usize];
        let modifier = if mapping.shifted { MOD_LEFT_SHIFT } else { 0 };
        Some(HidKeyboardReport::press(modifier, mapping.keycode))
    }

    /// Generate a sequence of HID reports for the given data.
    ///
    /// For each mappable byte: press report, then release report.
    /// After all data: terminator press + release (if configured).
    ///
    /// This follows the keyboard wedge pattern used by Zebra, Honeywell,
    /// and Datalogic scanners.
    pub fn map_to_reports<'b>(&'b self, data: &'b [u8]) -> ReportIterator<'b, 'a> {
        ReportIterator {
            mapper: self,
            data,
            pos: 0,
            state: ReportState::NextChar,
            pending_press: None,
        }
    }

    /// Count the number of HID reports that would be generated for the given data.
    ///
    /// Each mappable character produces 2 reports (press + release).
    /// The terminator adds 2 more reports if configured.
    #[must_use]
    pub fn report_count(&self, data: &[u8]) -> usize {
        let char_reports: usize = data.iter().filter(|&&b| self.is_mappable(b)).count() * 2;
        let terminator_reports = if self.terminator.keycode().is_some() {
            2
        } else {
            0
        };
        char_reports + terminator_reports
    }
}

/// State machine for report generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportState {
    NextChar,
    Release,
    TerminatorPress,
    TerminatorRelease,
    Done,
}

/// Iterator over HID keyboard reports for a barcode data sequence.
pub struct ReportIterator<'d, 'a> {
    mapper: &'d KeyMapper<'a>,
    data: &'d [u8],
    pos: usize,
    state: ReportState,
    pending_press: Option<HidKeyboardReport>,
}

impl<'d, 'a> Iterator for ReportIterator<'d, 'a> {
    type Item = HidKeyboardReport;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.state {
                ReportState::NextChar => {
                    // Find the next mappable character
                    while self.pos < self.data.len() {
                        let byte = self.data[self.pos];
                        self.pos += 1;
                        if let Some(press) = self.mapper.map_byte(byte) {
                            self.state = ReportState::Release;
                            self.pending_press = Some(press);
                            return Some(press);
                        }
                        // Skip unmappable bytes
                    }
                    // All characters processed, move to terminator
                    if self.mapper.terminator.keycode().is_some() {
                        self.state = ReportState::TerminatorPress;
                    } else {
                        self.state = ReportState::Done;
                    }
                }
                ReportState::Release => {
                    self.state = ReportState::NextChar;
                    self.pending_press = None;
                    return Some(HidKeyboardReport::release());
                }
                ReportState::TerminatorPress => {
                    self.state = ReportState::TerminatorRelease;
                    let keycode = self.mapper.terminator.keycode().unwrap();
                    return Some(HidKeyboardReport::press(0, keycode));
                }
                ReportState::TerminatorRelease => {
                    self.state = ReportState::Done;
                    return Some(HidKeyboardReport::release());
                }
                ReportState::Done => {
                    return None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate alloc;
    use alloc::vec::Vec;

    // ---- HidKeyboardReport tests ----

    #[test]
    fn test_release_report() {
        let r = HidKeyboardReport::release();
        assert_eq!(r.modifier, 0);
        assert_eq!(r.keycode, 0);
        assert_eq!(r.as_bytes(), [0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn test_press_report() {
        let r = HidKeyboardReport::press(MOD_LEFT_SHIFT, 0x04);
        assert_eq!(r.modifier, 0x02);
        assert_eq!(r.keycode, 0x04);
        assert_eq!(r.as_bytes(), [0x02, 0, 0x04, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn test_report_8_bytes() {
        let r = HidKeyboardReport::press(0, 0x28);
        let bytes = r.as_bytes();
        assert_eq!(bytes.len(), 8);
    }

    // ---- Terminator tests ----

    #[test]
    fn test_terminator_enter() {
        assert_eq!(Terminator::Enter.keycode(), Some(KEY_ENTER));
    }

    #[test]
    fn test_terminator_tab() {
        assert_eq!(Terminator::Tab.keycode(), Some(KEY_TAB));
    }

    #[test]
    fn test_terminator_none() {
        assert_eq!(Terminator::None.keycode(), None);
    }

    // ---- Layout tests ----

    #[test]
    fn test_us_layout_lowercase_a() {
        let m = US_ENGLISH[b'a' as usize];
        assert_eq!(m.keycode, 0x04);
        assert!(!m.shifted);
    }

    #[test]
    fn test_us_layout_uppercase_a() {
        let m = US_ENGLISH[b'A' as usize];
        assert_eq!(m.keycode, 0x04);
        assert!(m.shifted);
    }

    #[test]
    fn test_us_layout_digit_0() {
        let m = US_ENGLISH[b'0' as usize];
        assert_eq!(m.keycode, 0x27); // Per HID spec, '0' is 0x27
        assert!(!m.shifted);
    }

    #[test]
    fn test_us_layout_digit_1() {
        let m = US_ENGLISH[b'1' as usize];
        assert_eq!(m.keycode, 0x1E); // Per HID spec, '1' is 0x1E
        assert!(!m.shifted);
    }

    #[test]
    fn test_us_layout_space() {
        let m = US_ENGLISH[b' ' as usize];
        assert_eq!(m.keycode, 0x2C);
        assert!(!m.shifted);
    }

    #[test]
    fn test_us_layout_exclamation() {
        let m = US_ENGLISH[b'!' as usize];
        assert_eq!(m.keycode, 0x1E); // Shift+1
        assert!(m.shifted);
    }

    #[test]
    fn test_us_layout_at_sign() {
        let m = US_ENGLISH[b'@' as usize];
        assert_eq!(m.keycode, 0x1F); // Shift+2
        assert!(m.shifted);
    }

    #[test]
    fn test_us_layout_control_chars_unmapped() {
        // Control characters (0x00–0x1F) should be unmapped
        for i in 0u8..0x20 {
            assert_eq!(
                US_ENGLISH[i as usize].keycode, 0,
                "char 0x{:02x} should be unmapped",
                i
            );
        }
    }

    #[test]
    fn test_us_layout_all_lowercase_mapped() {
        for c in b'a'..=b'z' {
            let m = US_ENGLISH[c as usize];
            assert_ne!(m.keycode, 0, "char '{}' should be mapped", c as char);
            assert!(!m.shifted, "char '{}' should not need shift", c as char);
        }
    }

    #[test]
    fn test_us_layout_all_uppercase_mapped() {
        for c in b'A'..=b'Z' {
            let m = US_ENGLISH[c as usize];
            assert_ne!(m.keycode, 0, "char '{}' should be mapped", c as char);
            assert!(m.shifted, "char '{}' should need shift", c as char);
        }
    }

    #[test]
    fn test_us_layout_all_digits_mapped() {
        for c in b'0'..=b'9' {
            let m = US_ENGLISH[c as usize];
            assert_ne!(m.keycode, 0, "char '{}' should be mapped", c as char);
            assert!(!m.shifted, "char '{}' should not need shift", c as char);
        }
    }

    #[test]
    fn test_us_layout_upper_lower_same_keycode() {
        // Upper and lower case should use the same key code, just different shift
        for offset in 0u8..26 {
            let upper = US_ENGLISH[(b'A' + offset) as usize];
            let lower = US_ENGLISH[(b'a' + offset) as usize];
            assert_eq!(
                upper.keycode,
                lower.keycode,
                "'{}'/'{}' should have same keycode",
                (b'A' + offset) as char,
                (b'a' + offset) as char
            );
        }
    }

    // ---- KeyMapper tests ----

    #[test]
    fn test_map_byte_ascii() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        let report = mapper.map_byte(b'a').unwrap();
        assert_eq!(report.keycode, 0x04);
        assert_eq!(report.modifier, 0);
    }

    #[test]
    fn test_map_byte_shifted() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        let report = mapper.map_byte(b'A').unwrap();
        assert_eq!(report.keycode, 0x04);
        assert_eq!(report.modifier, MOD_LEFT_SHIFT);
    }

    #[test]
    fn test_map_byte_non_ascii() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        assert!(mapper.map_byte(0x80).is_none());
        assert!(mapper.map_byte(0xFF).is_none());
    }

    #[test]
    fn test_map_byte_control_char() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        assert!(mapper.map_byte(0x00).is_none());
        assert!(mapper.map_byte(0x0A).is_none()); // newline
        assert!(mapper.map_byte(0x0D).is_none()); // carriage return
    }

    // ---- ReportIterator tests ----

    #[test]
    fn test_reports_empty_data() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        let reports: Vec<_> = mapper.map_to_reports(b"").collect();
        assert_eq!(reports.len(), 0);
    }

    #[test]
    fn test_reports_empty_data_with_terminator() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::Enter);
        let reports: Vec<_> = mapper.map_to_reports(b"").collect();
        // Just terminator: press + release
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].keycode, KEY_ENTER);
        assert_eq!(reports[1], HidKeyboardReport::release());
    }

    #[test]
    fn test_reports_single_char() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        let reports: Vec<_> = mapper.map_to_reports(b"a").collect();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].keycode, 0x04); // 'a'
        assert_eq!(reports[0].modifier, 0);
        assert_eq!(reports[1], HidKeyboardReport::release());
    }

    #[test]
    fn test_reports_with_enter_terminator() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::Enter);
        let reports: Vec<_> = mapper.map_to_reports(b"1").collect();
        // '1' press + release + Enter press + release = 4
        assert_eq!(reports.len(), 4);
        assert_eq!(reports[0].keycode, 0x1E); // '1'
        assert_eq!(reports[1], HidKeyboardReport::release());
        assert_eq!(reports[2].keycode, KEY_ENTER);
        assert_eq!(reports[3], HidKeyboardReport::release());
    }

    #[test]
    fn test_reports_with_tab_terminator() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::Tab);
        let reports: Vec<_> = mapper.map_to_reports(b"x").collect();
        assert_eq!(reports.len(), 4);
        assert_eq!(reports[2].keycode, KEY_TAB);
    }

    #[test]
    fn test_reports_mixed_case() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        let reports: Vec<_> = mapper.map_to_reports(b"Hi").collect();
        assert_eq!(reports.len(), 4);
        // 'H' = shifted
        assert_eq!(reports[0].keycode, 0x0B); // H
        assert_eq!(reports[0].modifier, MOD_LEFT_SHIFT);
        // release
        assert_eq!(reports[1], HidKeyboardReport::release());
        // 'i' = unshifted
        assert_eq!(reports[2].keycode, 0x0C); // I
        assert_eq!(reports[2].modifier, 0);
        // release
        assert_eq!(reports[3], HidKeyboardReport::release());
    }

    #[test]
    fn test_reports_skip_non_ascii() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        let data = &[0x41, 0xFF, 0x42]; // 'A', 0xFF, 'B'
        let reports: Vec<_> = mapper.map_to_reports(data).collect();
        // 0xFF is skipped, so A + B = 4 reports
        assert_eq!(reports.len(), 4);
        assert_eq!(reports[0].keycode, 0x04); // 'A'
        assert_eq!(reports[2].keycode, 0x05); // 'B'
    }

    #[test]
    fn test_reports_typical_barcode() {
        // Typical EAN-13 barcode: "4006381333931"
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::Enter);
        let data = b"4006381333931";
        let reports: Vec<_> = mapper.map_to_reports(data).collect();
        // 13 digits * 2 + terminator * 2 = 28
        assert_eq!(reports.len(), 28);
    }

    #[test]
    fn test_report_count() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::Enter);
        assert_eq!(mapper.report_count(b"Hello"), 12); // 5*2 + 2
        assert_eq!(mapper.report_count(b""), 2); // just terminator

        let mapper_no_term = KeyMapper::new(&US_ENGLISH, Terminator::None);
        assert_eq!(mapper_no_term.report_count(b"Hi"), 4);
        assert_eq!(mapper_no_term.report_count(b""), 0);
    }

    #[test]
    fn test_report_count_matches_iterator() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::Enter);
        let data = b"Test123!@#";
        let expected = mapper.report_count(data);
        let actual = mapper.map_to_reports(data).count();
        assert_eq!(expected, actual);
    }

    // ---- Boot keyboard descriptor tests ----

    #[test]
    fn test_boot_keyboard_descriptor_valid() {
        // Must start with Usage Page (Generic Desktop) + Usage (Keyboard)
        assert_eq!(BOOT_KEYBOARD_REPORT_DESCRIPTOR[0], 0x05); // Usage Page
        assert_eq!(BOOT_KEYBOARD_REPORT_DESCRIPTOR[1], 0x01); // Generic Desktop
        assert_eq!(BOOT_KEYBOARD_REPORT_DESCRIPTOR[2], 0x09); // Usage
        assert_eq!(BOOT_KEYBOARD_REPORT_DESCRIPTOR[3], 0x06); // Keyboard
                                                              // Must end with End Collection
        assert_eq!(*BOOT_KEYBOARD_REPORT_DESCRIPTOR.last().unwrap(), 0xC0);
    }

    #[test]
    fn test_boot_keyboard_descriptor_length() {
        // Standard boot keyboard descriptor is 63 bytes
        assert_eq!(BOOT_KEYBOARD_REPORT_DESCRIPTOR.len(), 63);
    }

    // ---- Edge case tests ----

    #[test]
    fn test_map_byte_del_unmapped() {
        // DEL (0x7F) is not printable ASCII and must not be mapped
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        assert!(!mapper.is_mappable(0x7F));
        assert!(mapper.map_byte(0x7F).is_none());
    }

    #[test]
    fn test_all_printable_ascii_mapped() {
        // Every byte in 0x20–0x7E must have a non-zero keycode in US_ENGLISH
        for byte in 0x20u8..=0x7E {
            let m = US_ENGLISH[byte as usize];
            assert_ne!(
                m.keycode, 0,
                "printable ASCII 0x{:02x} ('{}') should be mapped",
                byte, byte as char
            );
        }
    }

    #[test]
    fn test_no_non_printable_ascii_mapped() {
        // 0x00–0x1F and 0x7F must all have keycode == 0
        for byte in 0x00u8..0x20 {
            assert_eq!(
                US_ENGLISH[byte as usize].keycode, 0,
                "control char 0x{:02x} should not be mapped",
                byte
            );
        }
        assert_eq!(
            US_ENGLISH[0x7F].keycode, 0,
            "DEL (0x7F) should not be mapped"
        );
    }

    #[test]
    fn test_report_count_with_unmappable_bytes() {
        // Non-ASCII bytes should be excluded from report count
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        let data = &[0x80, 0xFF, 0x00, 0x0A]; // all unmappable
        assert_eq!(mapper.report_count(data), 0);
    }

    #[test]
    fn test_report_count_mixed_mappable_unmappable() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::Enter);
        let data = &[b'A', 0xFF, b'B', 0x00]; // 2 mappable, 2 not
                                              // 2 chars * 2 reports + 2 terminator = 6
        assert_eq!(mapper.report_count(data), 6);
    }

    #[test]
    fn test_reports_all_unmappable_with_terminator() {
        // If all bytes are unmappable but terminator is set, only terminator emitted
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::Enter);
        let data = &[0xFF, 0x80, 0x00];
        let reports: Vec<_> = mapper.map_to_reports(data).collect();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].keycode, KEY_ENTER);
        assert_eq!(reports[1], HidKeyboardReport::release());
    }

    #[test]
    fn test_reports_all_unmappable_no_terminator() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        let data = &[0xFF, 0x80, 0x00];
        let reports: Vec<_> = mapper.map_to_reports(data).collect();
        assert_eq!(reports.len(), 0);
    }

    #[test]
    fn test_report_count_always_matches_iterator() {
        // Exhaustive consistency check: report_count must match actual iterator output
        // for all terminator variants and several data patterns
        let patterns: &[&[u8]] = &[
            b"",
            b"a",
            b"Hello World!",
            &[0xFF, 0x80],
            &[b'A', 0xFF, b'B'],
            b"0123456789",
            b"~!@#$%^&*()",
        ];
        for &term in &[Terminator::None, Terminator::Enter, Terminator::Tab] {
            let mapper = KeyMapper::new(&US_ENGLISH, term);
            for &data in patterns {
                let expected = mapper.report_count(data);
                let actual = mapper.map_to_reports(data).count();
                assert_eq!(
                    expected, actual,
                    "report_count mismatch for {:?} with {:?}",
                    data, term
                );
            }
        }
    }

    #[test]
    fn test_is_mappable_boundary_values() {
        let mapper = KeyMapper::new(&US_ENGLISH, Terminator::None);
        // 0x1F = last control char → not mappable
        assert!(!mapper.is_mappable(0x1F));
        // 0x20 = space → mappable
        assert!(mapper.is_mappable(0x20));
        // 0x7E = tilde → mappable
        assert!(mapper.is_mappable(0x7E));
        // 0x7F = DEL → not mappable
        assert!(!mapper.is_mappable(0x7F));
        // 0x80 = first non-ASCII → not mappable
        assert!(!mapper.is_mappable(0x80));
    }
}
