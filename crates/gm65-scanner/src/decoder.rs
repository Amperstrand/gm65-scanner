//! QR payload decoder.
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

/// Classification of a scanned QR payload.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PayloadType {
    /// Cashu V4 token (starts with `cashuB`).
    CashuV4,
    /// Cashu V3 token (starts with `cashuA`).
    CashuV3,
    /// UR (Uniform Resources) multi-part fragment (starts with `ur:`).
    UrFragment,
    /// HTTP/HTTPS URL.
    Url,
    /// UTF-8 text that is not a URL or known token format.
    PlainText,
    /// Non-UTF-8 binary data.
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

/// Classify a scanned payload by its byte prefix.
///
/// Checks for Cashu tokens, UR fragments, URLs, UTF-8 text, and binary data
/// in priority order.
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

/// Decode a raw scan payload into a classified result.
pub fn decode_payload(data: &[u8]) -> DecodedPayload {
    let payload_type = classify_payload(data);
    DecodedPayload {
        raw: data.to_vec(),
        payload_type,
    }
}

/// A decoded scan payload with classification metadata.
#[derive(Debug, Clone)]
pub struct DecodedPayload {
    /// Raw bytes from the scanner.
    pub raw: Vec<u8>,
    /// Detected payload type.
    pub payload_type: PayloadType,
}

impl DecodedPayload {
    /// Interpret the raw bytes as UTF-8 text.
    ///
    /// Returns `None` if the payload contains invalid UTF-8.
    pub fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.raw).ok()
    }
}

impl fmt::Display for DecodedPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({} bytes)", self.payload_type, self.raw.len())
    }
}

/// A parsed UR (Uniform Resources) fragment.
///
/// UR fragments follow the format: `ur:<type>/<index>-<total>/<hash>/<data>`.
#[derive(Debug, Clone)]
pub struct ParsedUrFragment {
    /// Fragment data type (e.g., "bytes", "crypto-psbt").
    pub ur_type: String,
    /// 1-based fragment index.
    pub index: u32,
    /// Total number of fragments in the sequence.
    pub total: u32,
    /// Hash identifying this multi-part sequence.
    pub hash: String,
    /// Fragment payload bytes.
    pub data: Vec<u8>,
}

/// Parse a UR fragment from raw bytes.
///
/// Returns `None` if the data is not a valid UR fragment string.
/// Fragment index must be >= 1 (index 0 is rejected as invalid).
pub fn parse_ur_fragment(data: &[u8]) -> Option<ParsedUrFragment> {
    let s = core::str::from_utf8(data).ok()?;
    if !s.starts_with("ur:") {
        return None;
    }

    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() < 4 {
        return None;
    }

    let scheme_and_type = parts.first()?;
    let ur_type = scheme_and_type.strip_prefix("ur:")?.to_lowercase();

    let index_total: Vec<&str> = parts[1].split('-').collect();
    if index_total.len() != 2 {
        return None;
    }

    let index = index_total[0].parse::<u32>().ok()?;
    let total = index_total[1].parse::<u32>().ok()?;
    let hash = parts[2].to_string();
    let data_str = parts[3..].join("/");
    let data = data_str.as_bytes().to_vec();

    Some(ParsedUrFragment {
        ur_type,
        index,
        total,
        hash,
        data,
    })
}

/// Reassembles multi-part UR fragment sequences into a single payload.
///
/// Feed fragments in any order. When all fragments are received, returns
/// the concatenated payload bytes.
///
/// # Examples
///
/// ```rust,ignore
/// let mut decoder = UrDecoder::new();
/// if let Some(data) = decoder.feed(fragment_bytes) {
///     // all fragments received, data is the complete payload
/// }
/// ```
#[derive(Debug)]
pub struct UrDecoder {
    total: Option<u32>,
    hash: Option<String>,
    fragments: Vec<Option<Vec<u8>>>,
    received: u32,
}

impl UrDecoder {
    /// Create a new decoder with no fragments received.
    pub fn new() -> Self {
        Self {
            total: None,
            hash: None,
            fragments: Vec::new(),
            received: 0,
        }
    }

    /// Reset the decoder, discarding all received fragments.
    pub fn reset(&mut self) {
        self.total = None;
        self.hash = None;
        self.fragments.clear();
        self.received = 0;
    }

    /// Feed a UR fragment. Returns the assembled payload when complete.
    ///
    /// Fragment index 0 is rejected. Duplicate fragments are ignored.
    /// Hash mismatches against the first fragment are rejected.
    pub fn feed(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        let fragment = parse_ur_fragment(data)?;

        if fragment.index == 0 {
            return None;
        }

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

    /// Return `(received, total)` fragment progress.
    pub fn progress(&self) -> (u32, u32) {
        (self.received, self.total.unwrap_or(0))
    }

    /// Return `true` if at least one fragment has been fed.
    pub fn is_active(&self) -> bool {
        self.total.is_some()
    }

    /// Return `true` if all fragments have been received and assembled.
    pub fn is_complete(&self) -> bool {
        self.total.map(|t| self.received == t).unwrap_or(false)
    }
}

impl Default for UrDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn test_classify_cashu_v4() {
        let data = b"cashuA...";
        assert_eq!(classify_payload(data), PayloadType::CashuV3);
    }

    #[test]
    fn test_classify_cashu_v3() {
        let data = b"cashuB...";
        assert_eq!(classify_payload(data), PayloadType::CashuV4);
    }

    #[test]
    fn test_classify_cashu_priority() {
        let data = b"cashuA...";
        assert_eq!(classify_payload(data), PayloadType::CashuV3);
    }

    #[test]
    fn test_classify_ur_fragment() {
        let data = b"ur:bytes/1-3/abc123/data";
        assert_eq!(classify_payload(data), PayloadType::UrFragment);
    }

    #[test]
    fn test_classify_url_http() {
        let data = b"http://example.com";
        assert_eq!(classify_payload(data), PayloadType::Url);
    }

    #[test]
    fn test_classify_url_https() {
        let data = b"https://example.com/path";
        assert_eq!(classify_payload(data), PayloadType::Url);
    }

    #[test]
    fn test_classify_plain_text() {
        let data = b"hello world";
        assert_eq!(classify_payload(data), PayloadType::PlainText);
    }

    #[test]
    fn test_classify_binary() {
        let data: &[u8] = &[0xFF, 0xFE, 0x00, 0x01];
        assert_eq!(classify_payload(data), PayloadType::Binary);
    }

    #[test]
    fn test_classify_empty() {
        let data: &[u8] = b"";
        assert_eq!(classify_payload(data), PayloadType::PlainText);
    }

    #[test]
    fn test_decode_payload() {
        let data = b"https://example.com";
        let payload = decode_payload(data);
        assert_eq!(payload.payload_type, PayloadType::Url);
        assert_eq!(payload.raw, data.to_vec());
    }

    #[test]
    fn test_decoded_payload_as_str() {
        let payload = decode_payload(b"hello");
        assert_eq!(payload.as_str(), Some("hello"));
    }

    #[test]
    fn test_decoded_payload_as_str_binary() {
        let payload = decode_payload(&[0xFF, 0xFE]);
        assert_eq!(payload.as_str(), None);
    }

    #[test]
    fn test_payload_type_display() {
        assert_eq!(format!("{}", PayloadType::CashuV4), "Cashu V4 Token");
        assert_eq!(format!("{}", PayloadType::CashuV3), "Cashu V3 Token");
        assert_eq!(format!("{}", PayloadType::UrFragment), "UR Fragment");
        assert_eq!(format!("{}", PayloadType::Url), "URL");
        assert_eq!(format!("{}", PayloadType::PlainText), "Plain Text");
        assert_eq!(format!("{}", PayloadType::Binary), "Binary Data");
    }

    #[test]
    fn test_parse_ur_fragment_valid() {
        let data = b"ur:bytes/1-3/abc123/payload-data";
        let frag = parse_ur_fragment(data).unwrap();
        assert_eq!(frag.ur_type, "bytes");
        assert_eq!(frag.index, 1);
        assert_eq!(frag.total, 3);
        assert_eq!(frag.hash, "abc123");
        assert_eq!(frag.data, b"payload-data");
    }

    #[test]
    fn test_parse_ur_fragment_no_prefix() {
        let data = b"bytes/1-3/abc123/data";
        assert!(parse_ur_fragment(data).is_none());
    }

    #[test]
    fn test_parse_ur_fragment_too_few_parts() {
        let data = b"ur:bytes/1-3";
        assert!(parse_ur_fragment(data).is_none());
    }

    #[test]
    fn test_parse_ur_fragment_invalid_index() {
        let data = b"ur:bytes/abc-3/hash/data";
        assert!(parse_ur_fragment(data).is_none());
    }

    #[test]
    fn test_parse_ur_fragment_case_insensitive_type() {
        let data = b"ur:BYTES/1-3/hash/data";
        let frag = parse_ur_fragment(data).unwrap();
        assert_eq!(frag.ur_type, "bytes");
    }

    #[test]
    fn test_ur_decoder_single_fragment() {
        let mut decoder = UrDecoder::new();
        let data = b"ur:crypto-psbt/1-1/hash/psbt-data";
        let result = decoder.feed(data);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), b"psbt-data");
        assert!(decoder.is_complete());
    }

    #[test]
    fn test_ur_decoder_multi_fragment() {
        let mut decoder = UrDecoder::new();

        let result = decoder.feed(b"ur:bytes/1-3/hash1/part-a");
        assert!(result.is_none());
        assert!(decoder.is_active());
        assert!(!decoder.is_complete());
        assert_eq!(decoder.progress(), (1, 3));

        let result = decoder.feed(b"ur:bytes/2-3/hash1/part-b");
        assert!(result.is_none());
        assert_eq!(decoder.progress(), (2, 3));

        let result = decoder.feed(b"ur:bytes/3-3/hash1/part-c");
        assert!(result.is_some());
        let assembled = result.unwrap();
        assert_eq!(assembled, b"part-apart-bpart-c");
        assert!(decoder.is_complete());
        assert_eq!(decoder.progress(), (3, 3));
    }

    #[test]
    fn test_ur_decoder_hash_mismatch() {
        let mut decoder = UrDecoder::new();
        decoder.feed(b"ur:bytes/1-2/hash1/part-a");
        let result = decoder.feed(b"ur:bytes/2-2/hash2/part-b");
        assert!(result.is_none());
    }

    #[test]
    fn test_ur_decoder_reset() {
        let mut decoder = UrDecoder::new();
        decoder.feed(b"ur:bytes/1-3/hash1/part-a");
        assert!(decoder.is_active());
        decoder.reset();
        assert!(!decoder.is_active());
        assert_eq!(decoder.progress(), (0, 0));
    }

    #[test]
    fn test_ur_decoder_duplicate_fragment() {
        let mut decoder = UrDecoder::new();
        decoder.feed(b"ur:bytes/1-2/hash1/part-a");
        let result = decoder.feed(b"ur:bytes/1-2/hash1/part-a");
        assert!(result.is_none());
        assert_eq!(decoder.progress(), (1, 2));
    }

    #[test]
    fn test_ur_decoder_index_zero_rejected() {
        let mut decoder = UrDecoder::new();
        let result = decoder.feed(b"ur:bytes/0-3/hash1/part-a");
        assert!(result.is_none());
        assert!(!decoder.is_active());
    }

    #[test]
    fn test_ur_decoder_mismatched_total_ignored() {
        let mut decoder = UrDecoder::new();
        decoder.feed(b"ur:bytes/1-3/hash1/part-a");
        let result = decoder.feed(b"ur:bytes/2-5/hash1/part-b");
        assert!(result.is_none());
        assert_eq!(decoder.progress(), (2, 3));
    }
}
