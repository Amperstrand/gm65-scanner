//! QR payload decoder
//!
//! Generic payload classification for scanned QR data.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

const CASHU_V4_PREFIX: &[u8] = b"cashuB";
const CASHU_V3_PREFIX: &[u8] = b"cashuA";
const UR_PREFIX: &[u8] = b"ur:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadType {
    CashuV4,
    CashuV3,
    UrFragment,
    Url,
    PlainText,
    Binary,
}

impl fmt::Display for PayloadType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            PayloadType::CashuV4 => write!(f, "Cashu V4 Token"),
            PayloadType::CashuV3 => write!(f, "Cashu V3 Token"),
            PayloadType::UrFragment => write!(f, "UR Fragment"),
            PayloadType::Url => write!(f, "URL"),
            PayloadType::PlainText => write!(f, "Plain Text"),
            PayloadType::Binary => write!(f, "Binary Data"),
        }
    }
}

pub fn classify_payload(data: &[u8]) -> PayloadType {
    if data.starts_with(CASHU_V4_PREFIX) {
        return PayloadType::CashuV4;
    }
    if data.starts_with(CASHU_V3_PREFIX) {
        return PayloadType::CashuV3;
    }
    if data.starts_with(UR_PREFIX) {
        return PayloadType::UrFragment;
    }
    if let Ok(text) = core::str::from_utf8(data) {
        if text.starts_with("http://") || text.starts_with("https://") {
            return PayloadType::Url;
        }
        return PayloadType::PlainText;
    }
    PayloadType::Binary
}

pub fn decode_payload(data: &[u8]) -> DecodedPayload {
    let payload_type = classify_payload(data);
    DecodedPayload {
        raw: data.to_vec(),
        payload_type,
    }
}

#[derive(Debug, Clone)]
pub struct DecodedPayload {
    pub raw: Vec<u8>,
    pub payload_type: PayloadType,
}

impl DecodedPayload {
    pub fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.raw).ok()
    }
}

impl fmt::Display for DecodedPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({} bytes)",
            self.payload_type,
            self.raw.len()
        )
    }
}
