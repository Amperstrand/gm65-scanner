pub fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        let boundary = s.floor_char_boundary(max_len);
        &s[..boundary]
    }
}

pub fn format_u32_len(len: usize) -> heapless::String<16> {
    let mut s = heapless::String::new();
    if len == 0 {
        let _ = s.push('0');
    } else if len < 10 {
        let _ = s.push((b'0' + len as u8) as char);
    } else if len < 100 {
        let _ = s.push((b'0' + (len / 10) as u8) as char);
        let _ = s.push((b'0' + (len % 10) as u8) as char);
    } else if len < 1000 {
        let _ = s.push((b'0' + (len / 100) as u8) as char);
        let _ = s.push((b'0' + ((len / 10) % 10) as u8) as char);
        let _ = s.push((b'0' + (len % 10) as u8) as char);
    } else {
        let mut n = len;
        let mut digits = [0u8; 8];
        let mut i = 0;
        while n > 0 && i < 8 {
            digits[i] = (n % 10) as u8;
            n /= 10;
            i += 1;
        }
        for j in (0..i).rev() {
            let _ = s.push(digits[j] as char);
        }
    }
    let _ = s.push_str(" bytes");
    s
}

pub fn format_byte(b: u8) -> heapless::String<4> {
    let mut s = heapless::String::new();
    let hex = b"0123456789ABCDEF";
    let _ = s.push(hex[(b >> 4) as usize] as char);
    let _ = s.push(hex[(b & 0x0F) as usize] as char);
    s
}
