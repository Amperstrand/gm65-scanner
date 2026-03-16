//! Scan buffer for QR scanner data
//!
//! Handles buffering and incoming UART data and detecting EOL-terminated payloads.

extern crate alloc;

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
        if self.data[self.len - 1] == b'\r' {
            return true;
        }
        if self.data[self.len - 1] == b'\n' {
            return true;
        }
        false
    }

    pub fn data_without_eol(&self) -> &[u8] {
        let mut end = self.len;
        if end > 0 && self.data[end - 1] == b'\r' {
            end -= 1;
        }
        if end > 0 && self.data[end - 1] == b'\n' {
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
