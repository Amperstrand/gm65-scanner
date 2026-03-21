use alloc::vec::Vec;
use usbd_hid::descriptor::{generator_prelude::*, KeyboardReport, SerializedDescriptor};

#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = BARCODE_SCANNER, usage = 0x01) = {
        (usage_min = 0x00, usage_max = 0xFF) = {
            #[item_settings(data,variable,absolute)] barcode_data=input;
        };
    }
)]
pub struct BarcodeScannerReport {
    pub barcode_data: [u8; 32],
}

pub const HID_KBD_POLL_MS: u8 = 10;
pub const HID_POS_POLL_MS: u8 = 10;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HidMode {
    KeyboardWedge,
    PosScanner,
}

pub struct KeyboardWedgeState {
    text: Vec<u8>,
    pos: usize,
    phase: KeyPhase,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyPhase {
    Idle,
    Press,
    Release,
    Done,
}

impl Default for KeyboardWedgeState {
    fn default() -> Self {
        Self {
            text: Vec::new(),
            pos: 0,
            phase: KeyPhase::Idle,
        }
    }
}

impl KeyboardWedgeState {
    pub fn start(&mut self, data: &[u8]) {
        self.text.clear();
        let copy_len = data.len().min(255);
        self.text.extend_from_slice(&data[..copy_len]);
        self.pos = 0;
        self.phase = KeyPhase::Press;
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.phase, KeyPhase::Idle | KeyPhase::Done)
    }

    pub fn send_next<B: usb_device::bus::UsbBus>(
        &mut self,
        hid: &mut usbd_hid::hid_class::HIDClass<'_, B>,
    ) {
        match self.phase {
            KeyPhase::Idle | KeyPhase::Done => {
                self.phase = KeyPhase::Idle;
                return;
            }
            KeyPhase::Press => {
                if self.pos >= self.text.len() {
                    let _ = hid.push_input(&KeyboardReport::default());
                    self.phase = KeyPhase::Done;
                    return;
                }
                let byte = self.text[self.pos];
                match ascii_to_keycode(byte) {
                    Some((keycode, modifier)) => {
                        let _ = hid.push_input(&KeyboardReport {
                            modifier,
                            reserved: 0,
                            leds: 0,
                            keycodes: [keycode, 0, 0, 0, 0, 0],
                        });
                        self.phase = KeyPhase::Release;
                    }
                    None => {
                        self.pos += 1;
                    }
                }
            }
            KeyPhase::Release => {
                let _ = hid.push_input(&KeyboardReport::default());
                self.pos += 1;
                self.phase = KeyPhase::Press;
            }
        }
    }
}

pub fn ascii_to_keycode(byte: u8) -> Option<(u8, u8)> {
    match byte {
        b'a'..=b'z' => Some((4 + (byte - b'a'), 0)),
        b'A'..=b'Z' => Some((4 + (byte - b'A'), 0x02)),
        b'1'..=b'9' => Some((0x1E + (byte - b'1'), 0)),
        b'0' => Some((0x27, 0)),
        b'\n' | b'\r' => Some((0x28, 0)),
        b' ' => Some((0x2C, 0)),
        b'-' => Some((0x2D, 0)),
        b'=' => Some((0x2E, 0)),
        b'[' => Some((0x2F, 0)),
        b']' => Some((0x30, 0)),
        b'\\' => Some((0x31, 0)),
        b';' => Some((0x33, 0)),
        b'\'' => Some((0x34, 0)),
        b'`' => Some((0x35, 0)),
        b',' => Some((0x36, 0)),
        b'.' => Some((0x37, 0)),
        b'/' => Some((0x38, 0)),
        b'_' => Some((0x2D, 0x02)),
        b'+' => Some((0x2E, 0x02)),
        b'{' => Some((0x2F, 0x02)),
        b'}' => Some((0x30, 0x02)),
        b'|' => Some((0x31, 0x02)),
        b':' => Some((0x33, 0x02)),
        b'"' => Some((0x34, 0x02)),
        b'~' => Some((0x35, 0x02)),
        b'<' => Some((0x36, 0x02)),
        b'>' => Some((0x37, 0x02)),
        b'?' => Some((0x38, 0x02)),
        b'\t' => Some((0x2B, 0)),
        b'\x08' => Some((0x2A, 0)),
        _ => None,
    }
}
