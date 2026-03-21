//! QR payload decoder
//!
//! Generic payload classification for scanned QR data.
//! Includes UR (Uniform Resources) multi-fragment decoding.

extern crate alloc;

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

const CASHU_V4_PREFIX: &[u8] = b"cashuB";
const CASHU_V3_PREFIX: &[u8] = b"cashuA";
const UR_PREFIX: &[u8] = b"ur:";

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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
        match self {
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
        write!(f, "{} ({} bytes)", self.payload_type, self.raw.len())
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ParsedUrFragment {
    pub ur_type: String,
    pub index: u32,
    pub total: u32,
    pub hash: String,
    pub data: Vec<u8>,
}

pub fn parse_ur_fragment(data: &[u8]) -> Option<ParsedUrFragment> {
    let s = core::str::from_utf8(data).ok()?;
    if !s.starts_with("ur:") {
        return None;
    }

    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() < 5 {
        return None;
    }

    let ur_type = parts.get(1)?.to_lowercase();
    let index_total: Vec<&str> = parts[2].split('-').collect();
    if index_total.len() != 2 {
        return None;
    }

    let index = index_total[0].parse::<u32>().ok()?;
    let total = index_total[1].parse::<u32>().ok()?;
    let hash = parts[3].to_string();
    let data_str = parts[4..].join("/");
    let data = data_str.as_bytes().to_vec();

    Some(ParsedUrFragment {
        ur_type,
        index,
        total,
        hash,
        data,
    })
}

#[derive(Debug)]
pub struct UrDecoder {
    total: Option<u32>,
    hash: Option<String>,
    fragments: Vec<Option<Vec<u8>>>,
    received: u32,
}

impl UrDecoder {
    pub fn new() -> Self {
        Self {
            total: None,
            hash: None,
            fragments: Vec::new(),
            received: 0,
        }
    }

    pub fn reset(&mut self) {
        self.total = None;
        self.hash = None;
        self.fragments.clear();
        self.received = 0;
    }

    pub fn feed(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        let fragment = parse_ur_fragment(data)?;

        if self.total.is_none() {
            self.total = Some(fragment.total);
            self.hash = Some(fragment.hash.clone());
            self.fragments = vec![None; fragment.total as usize];
        }

        if self.hash.as_ref() != Some(&fragment.hash) {
            return None;
        }

        let idx = (fragment.index - 1) as usize;
        if idx < self.fragments.len() && self.fragments[idx].is_none() {
            self.fragments[idx] = Some(fragment.data);
            self.received += 1;
        }

        if self.received == self.total? {
            let mut result = Vec::new();
            for frag in &self.fragments {
                match frag {
                    Some(d) => result.extend_from_slice(d),
                    None => return None,
                }
            }
            return Some(result);
        }

        None
    }

    pub fn progress(&self) -> (u32, u32) {
        (self.received, self.total.unwrap_or(0))
    }

    pub fn is_active(&self) -> bool {
        self.total.is_some()
    }

    pub fn is_complete(&self) -> bool {
        self.total.map(|t| self.received == t).unwrap_or(false)
    }
}

impl Default for UrDecoder {
    fn default() -> Self {
        Self::new()
    }
}
