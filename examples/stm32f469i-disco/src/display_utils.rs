const MAX_DIGITS: usize = 8;

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
        let mut digits = [0u8; MAX_DIGITS];
        let mut i = 0;
        while n > 0 && i < MAX_DIGITS {
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

const HEX_TABLE: &[u8; 16] = b"0123456789ABCDEF";

pub fn format_byte(b: u8) -> heapless::String<4> {
    let mut s = heapless::String::new();
    let _ = s.push(HEX_TABLE[(b >> 4) as usize] as char);
    let _ = s.push(HEX_TABLE[(b & 0x0F) as usize] as char);
    s
}

/// Split text into lines that fit within `chars_per_line` characters.
/// Returns line boundaries as (start, end) byte offsets into the original string.
pub fn wrap_text_offsets(text: &str, chars_per_line: usize) -> heapless::Vec<(usize, usize), 32> {
    let mut lines = heapless::Vec::new();
    let mut offset = 0;
    while offset < text.len() {
        let remaining = text.len() - offset;
        let take = core::cmp::min(chars_per_line, remaining);
        let boundary = text.floor_char_boundary(offset + take);
        let _ = lines.push((offset, boundary));
        offset = boundary;
    }
    lines
}

/// Calculate the visible portion of centered text on a display.
/// Returns (start_byte, end_byte) into the text that fits within display_width pixels,
/// given font_width pixels per character and center_x as the center point.
pub fn centered_visible_range(
    text_len_chars: usize,
    font_width: usize,
    display_width: usize,
    center_x: usize,
) -> (usize, usize) {
    let text_width = text_len_chars * font_width;
    let left_edge = center_x.saturating_sub(text_width / 2);
    let right_edge = center_x + text_width / 2;

    if left_edge >= display_width || right_edge == 0 {
        return (0, 0);
    }

    let visible_left = left_edge.saturating_sub(0);
    let chars_hidden_left = visible_left / font_width;
    let visible_width = display_width.saturating_sub(visible_left.max(0));
    let chars_visible = visible_width / font_width;

    let start = chars_hidden_left.min(text_len_chars);
    let end = (start + chars_visible).min(text_len_chars);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate_str("hello world", 5), "hello");
    }

    #[test]
    fn test_wrap_short_text() {
        let lines = wrap_text_offsets("hello", 10);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], (0, 5));
    }

    #[test]
    fn test_wrap_exact_fit() {
        let lines = wrap_text_offsets("hello world", 5);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], (0, 5));
        assert_eq!(lines[1], (5, 10));
        assert_eq!(lines[2], (10, 11));
    }

    #[test]
    fn test_wrap_url() {
        let url = "https://en.wikipedia.org/wiki/QR_code";
        let lines = wrap_text_offsets(url, 44);
        assert!(lines.len() >= 1);
        let mut reconstructed = String::new();
        for (start, end) in lines.iter() {
            reconstructed.push_str(&url[*start..*end]);
        }
        assert_eq!(reconstructed, url);
    }

    #[test]
    fn test_wrap_preserves_all_text() {
        let text = "A".repeat(200);
        let lines = wrap_text_offsets(&text, 44);
        let total: usize = lines.iter().map(|(s, e)| e - s).sum();
        assert_eq!(total, 200);
    }

    #[test]
    fn test_centered_fully_visible() {
        // 20 chars * 10px = 200px, centered at 240
        // left=140, right=340, display=480
        let (start, end) = centered_visible_range(20, 10, 480, 240);
        assert_eq!(start, 0);
        assert_eq!(end, 20);
    }

    #[test]
    fn test_centered_partially_clipped() {
        // 50 chars * 10px = 500px, centered at 240
        // left=240-250=-10→0, right=240+250=490
        // chars_hidden_left = 0, visible_width = 480, chars_visible = 48
        let (start, end) = centered_visible_range(50, 10, 480, 240);
        assert_eq!(start, 0);
        assert_eq!(end, 48);
    }

    #[test]
    fn test_centered_ipedia_case() {
        // Simulate "https://wikipedia.org" (20 chars)
        let text = "https://wikipedia.org";
        let (start, end) = centered_visible_range(text.len(), 10, 480, 240);
        let visible = &text[start..end];
        assert_eq!(visible, "https://wikipedia.org");
    }

    #[test]
    fn test_centered_long_url() {
        // 55 chars * 10px = 550px, centered at 240
        let text = "https://en.wikipedia.org/wiki/QR_code#History_of_QR";
        let (start, end) = centered_visible_range(text.len(), 10, 480, 240);
        let visible = &text[start..end];
        assert!(visible.contains("wikipedia"));
    }

    #[test]
    fn test_format_bytes_short() {
        assert_eq!(format_u32_len(0).as_str(), "0 bytes");
        assert_eq!(format_u32_len(5).as_str(), "5 bytes");
    }

    #[test]
    fn test_format_bytes_large() {
        assert_eq!(format_u32_len(42).as_str(), "42 bytes");
        assert_eq!(format_u32_len(1234).as_str(), "1234 bytes");
    }

    use std::string::String;
}
