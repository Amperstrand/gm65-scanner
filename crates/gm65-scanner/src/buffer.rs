//! Scan buffer for QR scanner data
//!
//! Handles buffering incoming UART data and detecting EOL-terminated payloads.

pub const MAX_SCAN_SIZE: usize = 2048;

pub struct ScanBuffer {
    data: [u8; MAX_SCAN_SIZE],
    len: usize,
}

impl ScanBuffer {
    pub const fn new() -> Self {
        Self {
            data: [0u8; MAX_SCAN_SIZE],
            len: 0,
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn push(&mut self, byte: u8) -> bool {
        if self.len >= MAX_SCAN_SIZE {
            return false;
        }
        self.data[self.len] = byte;
        self.len += 1;
        true
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn has_eol(&self) -> bool {
        if self.len == 0 {
            return false;
        }
        if self.len >= 2 && self.data[self.len - 2] == b'\r' && self.data[self.len - 1] == b'\n' {
            return true;
        }
        if self.data[self.len - 1] == b'\r' || self.data[self.len - 1] == b'\n' {
            return true;
        }
        false
    }

    pub fn data_without_eol(&self) -> &[u8] {
        let mut end = self.len;
        if end > 0 && self.data[end - 1] == b'\n' {
            end -= 1;
        }
        if end > 0 && self.data[end - 1] == b'\r' {
            end -= 1;
        }
        &self.data[..end]
    }
}

impl Default for ScanBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let buf = ScanBuffer::new();
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
        assert_eq!(buf.as_slice(), &[]);
    }

    #[test]
    fn test_default() {
        let buf = ScanBuffer::default();
        assert!(buf.is_empty());
    }

    #[test]
    fn test_push_and_len() {
        let mut buf = ScanBuffer::new();
        assert!(buf.push(b'a'));
        assert_eq!(buf.len(), 1);
        assert!(!buf.is_empty());
        assert!(buf.push(b'b'));
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn test_as_slice() {
        let mut buf = ScanBuffer::new();
        buf.push(b'h');
        buf.push(b'i');
        assert_eq!(buf.as_slice(), &[b'h', b'i']);
    }

    #[test]
    fn test_clear() {
        let mut buf = ScanBuffer::new();
        buf.push(b'x');
        buf.push(b'y');
        buf.clear();
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_buffer_overflow() {
        let mut buf = ScanBuffer::new();
        for i in 0..MAX_SCAN_SIZE {
            assert!(buf.push(i as u8));
        }
        assert_eq!(buf.len(), MAX_SCAN_SIZE);
        assert!(!buf.push(0xFF));
        assert_eq!(buf.len(), MAX_SCAN_SIZE);
    }

    #[test]
    fn test_has_eol_empty() {
        let buf = ScanBuffer::new();
        assert!(!buf.has_eol());
    }

    #[test]
    fn test_has_eol_crlf() {
        let mut buf = ScanBuffer::new();
        buf.push(b'd');
        buf.push(b'\r');
        buf.push(b'\n');
        assert!(buf.has_eol());
    }

    #[test]
    fn test_has_eol_cr_only() {
        let mut buf = ScanBuffer::new();
        buf.push(b'd');
        buf.push(b'\r');
        assert!(buf.has_eol());
    }

    #[test]
    fn test_has_eol_lf_only() {
        let mut buf = ScanBuffer::new();
        buf.push(b'd');
        buf.push(b'\n');
        assert!(buf.has_eol());
    }

    #[test]
    fn test_has_eol_no_eol() {
        let mut buf = ScanBuffer::new();
        buf.push(b'h');
        buf.push(b'e');
        buf.push(b'l');
        buf.push(b'l');
        buf.push(b'o');
        assert!(!buf.has_eol());
    }

    #[test]
    fn test_data_without_eol_crlf() {
        let mut buf = ScanBuffer::new();
        buf.push(b'd');
        buf.push(b'\r');
        buf.push(b'\n');
        assert_eq!(buf.data_without_eol(), &[b'd']);
    }

    #[test]
    fn test_data_without_eol_cr_only() {
        let mut buf = ScanBuffer::new();
        buf.push(b'd');
        buf.push(b'\r');
        assert_eq!(buf.data_without_eol(), &[b'd']);
    }

    #[test]
    fn test_data_without_eol_lf_only() {
        let mut buf = ScanBuffer::new();
        buf.push(b'd');
        buf.push(b'\n');
        assert_eq!(buf.data_without_eol(), &[b'd']);
    }

    #[test]
    fn test_data_without_eol_no_eol() {
        let mut buf = ScanBuffer::new();
        buf.push(b'h');
        buf.push(b'i');
        assert_eq!(buf.data_without_eol(), &[b'h', b'i']);
    }

    #[test]
    fn test_data_without_eol_empty() {
        let buf = ScanBuffer::new();
        assert_eq!(buf.data_without_eol(), &[]);
    }
}
