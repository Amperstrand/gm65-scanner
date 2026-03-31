//! Shared test helpers for sync and async driver tests.

use alloc::vec::Vec;

/// Inner state shared between sync and async mock UARTs.
pub struct MockInner {
    pub read_queue: Vec<u8>,
    pub written: Vec<u8>,
    pub pending_responses: Vec<Vec<u8>>,
}

/// Build a 7-byte success response carrying the given value.
pub fn success_response(value: u8) -> [u8; 7] {
    [0x02, 0x00, 0x00, 0x01, value, 0x33, 0x31]
}

/// Build the full response sequence for a successful `init()` call.
///
/// Returns the byte buffer and its filled length. Each response is 7 bytes.
/// Split into 7-byte chunks when constructing a mock UART.
pub fn init_response_sequence() -> ([u8; 7 * 20], usize) {
    let mut buf = [0u8; 7 * 20];
    let mut idx = 0usize;

    let r = |buf: &mut [u8], idx: &mut usize, v: u8| {
        let resp = success_response(v);
        buf[*idx..*idx + 7].copy_from_slice(&resp);
        *idx += 7;
    };

    // DrainAndRead (probe): SerialOutput = 0xA0
    r(&mut buf, &mut idx, 0xA0);
    // ReadRegister: SerialOutput = 0xA0 (no fix needed)
    r(&mut buf, &mut idx, 0xA0);
    // WriteRegister: Settings = CMD_MODE (0x81) -> ack
    r(&mut buf, &mut idx, 0x81);

    // Config sequence: 5 registers, read-compare-write-verify
    // Each register: read (current value) + write (if needed) + verify
    // Values are already at target, so only read + verify needed per register
    let targets: [u8; 5] = [0x00, 0x01, 0x85, 0x01, 0x01];
    for _ in 0..5 {
        r(&mut buf, &mut idx, 0xFF);
    }
    for t in &targets {
        r(&mut buf, &mut idx, *t);
    }
    for t in &targets {
        r(&mut buf, &mut idx, *t);
    }

    // Version check: 0x87 (no raw mode fix needed)
    r(&mut buf, &mut idx, 0x87);
    // Save settings ack
    r(&mut buf, &mut idx, 0x00);

    (buf, idx)
}
