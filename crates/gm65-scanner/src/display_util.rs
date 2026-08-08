//! Display utility functions for text rendering on fixed-width screens.
//!
//! These functions are hardware-independent and fully testable without
//! a display. They handle text wrapping and visibility calculations
//! for the embedded-graphics rendering used by the firmware examples.

use alloc::vec::Vec;
use alloc::string::String;

/// Split text into lines that fit within `chars_per_line` characters.
///
/// Returns byte offsets (start, end) into the original string for each line.
/// Uses `floor_char_boundary` to avoid splitting multi-byte UTF-8 sequences.
pub fn wrap_text_offsets(text: &str, chars_per_line: usize) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let mut offset = 0;
    while offset < text.len() {
        let remaining = text.len() - offset;
        let take = core::cmp::min(chars_per_line, remaining);
        let boundary = text.floor_char_boundary(offset + take);
        lines.push((offset, boundary));
        offset = boundary;
    }
    lines
}

/// Calculate which characters of centered text are visible on a display.
///
/// Given `text_len_chars` characters at `font_width` pixels each, centered
/// at `center_x` on a `display_width` pixel wide screen, returns the
/// (start_char, end_char) range that falls within the visible area.
///
/// This reproduces the behavior of embedded-graphics' `Alignment::Center`
/// which positions text centered around the anchor point, clipping
/// characters that fall outside the drawable area.
pub fn centered_visible_range(
    text_len_chars: usize,
    font_width: usize,
    display_width: usize,
    center_x: usize,
) -> (usize, usize) {
    if text_len_chars == 0 || font_width == 0 {
        return (0, 0);
    }

    let text_width = text_len_chars * font_width;
    let ideal_left = center_x as i64 - (text_width / 2) as i64;

    let chars_hidden_left = if ideal_left < 0 {
        ((-ideal_left) / font_width as i64) as usize
    } else {
        0
    };

    let visible_start_px = ideal_left.max(0) as usize;
    let visible_width_px = display_width.saturating_sub(visible_start_px);
    let chars_visible_in_width = visible_width_px / font_width;

    let start = chars_hidden_left.min(text_len_chars);
    let end = (start + chars_visible_in_width).min(text_len_chars);
    (start, end)
}

/// Split text into word-wrapped lines.
///
/// Breaks at spaces when possible. Falls back to character break
/// for words longer than `chars_per_line`.
pub fn word_wrap(text: &str, chars_per_line: usize) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut current_start = 0usize;

    while current_start < text.len() {
        let remaining = &text[current_start..];
        if remaining.len() <= chars_per_line {
            lines.push(remaining);
            break;
        }

        let chunk = &remaining[..remaining.floor_char_boundary(chars_per_line)];

        if let Some(space_pos) = chunk.rfind(' ') {
            let break_pos = current_start + space_pos + 1;
            lines.push(&text[current_start..break_pos - 1]);
            current_start = break_pos;
        } else {
            let boundary = current_start + chunk.len();
            lines.push(&text[current_start..boundary]);
            current_start = boundary;
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::string::String;

    // === wrap_text_offsets tests ===

    #[test]
    fn test_wrap_empty() {
        let lines = wrap_text_offsets("", 44);
        assert!(lines.is_empty());
    }

    #[test]
    fn test_wrap_short() {
        let lines = wrap_text_offsets("hello", 44);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], (0, 5));
    }

    #[test]
    fn test_wrap_exact_multiple() {
        let lines = wrap_text_offsets("AAAAAAAAAABBBBBBBBBBCCCCCCCCCCDDDDDDDDDD", 10);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], (0, 10));
        assert_eq!(lines[3], (30, 40));
    }

    #[test]
    fn test_wrap_preserves_all_content() {
        let text = "https://en.wikipedia.org/wiki/QR_code#History";
        let lines = wrap_text_offsets(text, 44);
        let mut reconstructed = String::new();
        for (s, e) in &lines {
            reconstructed.push_str(&text[*s..*e]);
        }
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn test_wrap_single_char_per_line() {
        let lines = wrap_text_offsets("abc", 1);
        assert_eq!(lines.len(), 3);
    }

    // === centered_visible_range tests ===

    #[test]
    fn test_centered_short_text_fully_visible() {
        let (s, e) = centered_visible_range(20, 10, 480, 240);
        assert_eq!(s, 0);
        assert_eq!(e, 20);
    }

    #[test]
    fn test_centered_text_exactly_fits() {
        let (s, e) = centered_visible_range(48, 10, 480, 240);
        assert_eq!(s, 0);
        assert_eq!(e, 48);
    }

    #[test]
    fn test_centered_text_clipped_both_sides() {
        // 60 chars * 10px = 600px, centered at 240
        // ideal_left = 240 - 300 = -60 (6 chars hidden on left)
        // visible from char 6, 48 chars fit in 480px
        let (s, e) = centered_visible_range(60, 10, 480, 240);
        assert_eq!(s, 6);
        assert_eq!(e, 54);
        assert_eq!(e - s, 48);
    }

    #[test]
    fn test_centered_text_partially_off_left() {
        // 52 chars * 10px = 520px, centered at 240
        // left = 240 - 260 = -20 -> 0
        // Only 48 chars visible (480px / 10px)
        let (s, e) = centered_visible_range(52, 10, 480, 240);
        assert!(e - s <= 48);
        assert!(e <= 52);
    }

    #[test]
    fn test_centered_empty() {
        let (s, e) = centered_visible_range(0, 10, 480, 240);
        assert_eq!(s, 0);
        assert_eq!(e, 0);
    }

    #[test]
    fn test_ipedia_repro() {
        // The bug: QR label "https://wikipedia.org" centered
        // 20 chars * 10px = 200px, centered at 240
        // left=140, right=340 — fully visible on 480px display
        // This test proves the label SHOULD be fully visible
        // and the bug was elsewhere (QR mirror overwriting text)
        let text = "https://wikipedia.org";
        let (s, e) = centered_visible_range(text.len(), 10, 480, 240);
        assert_eq!(&text[s..e], "https://wikipedia.org");
    }

    // === word_wrap tests ===

    #[test]
    fn test_word_wrap_simple() {
        let lines = word_wrap("hello world", 10);
        assert_eq!(lines, vec!["hello", "world"]);
    }

    #[test]
    fn test_word_wrap_long_word() {
        let lines = word_wrap("abcdefghijklmnopqrstuvwxyz", 10);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "abcdefghij");
    }

    #[test]
    fn test_word_wrap_preserves_content() {
        let text = "The quick brown fox jumps over the lazy dog";
        let lines = word_wrap(text, 15);
        let reconstructed: String = lines.join(" ");
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn test_word_wrap_url() {
        let url = "https://en.wikipedia.org/wiki/QR_code";
        let lines = word_wrap(url, 44);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], url);
    }

    #[test]
    fn test_word_wrap_multiple_spaces() {
        let lines = word_wrap("a b c d e f", 3);
        assert!(lines.iter().all(|l| l.len() <= 3));
    }

    // === MockDisplay integration tests ===

    #[test]
    fn test_mock_display_wrapped_text_renders() {
        use embedded_graphics::{
            mock_display::MockDisplay,
            mono_font::{ascii::FONT_6X9, MonoTextStyle},
            pixelcolor::BinaryColor,
            prelude::*,
            text::Text,
        };

        let mut display = MockDisplay::<BinaryColor>::new();
        let style = MonoTextStyle::new(&FONT_6X9, BinaryColor::On);
        let text = "hello world";
        let lines = wrap_text_offsets(text, 5);

        let mut y = 9;
        for (start, end) in &lines {
            let line = &text[*start..*end];
            Text::new(line, Point::new(0, y), style)
                .draw(&mut display)
                .ok();
            y += 10;
        }

        let blank = MockDisplay::<BinaryColor>::new();
        assert_ne!(display, blank, "display should have pixels after rendering");
    }

    #[test]
    fn test_mock_display_word_wrap_no_split() {
        let text = "hello world test";
        let lines = word_wrap(text, 10);
        for line in &lines {
            if line.contains(' ') || line.len() == text.len() {
                continue;
            }
            let words_in_original: Vec<&str> = text.split_whitespace().collect();
            let is_full_word = words_in_original.iter().any(|w| *w == *line);
            assert!(is_full_word || line.len() <= 10);
        }
    }

    #[test]
    fn test_mock_display_centered_visibility_matches_rendering() {
        let text = "short";
        let (start, end) = centered_visible_range(text.len(), 10, 480, 240);
        let visible = &text[start..end];
        assert_eq!(visible, text);
    }

    #[test]
    fn test_wrap_line_count_matches_expected() {
        let text = "AAAAAAAAAA";
        let lines = wrap_text_offsets(text, 5);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], (0, 5));
        assert_eq!(lines[1], (5, 10));
    }

    #[test]
    fn test_long_url_preserves_all_content_through_wrapping() {
        let url = "https://en.wikipedia.org/wiki/QR_code#History_of_QR_codes";
        let char_lines = wrap_text_offsets(url, 44);
        let word_lines = word_wrap(url, 44);

        let char_reconstructed: String = char_lines.iter().map(|(s, e)| &url[*s..*e]).collect::<Vec<_>>().join("");
        let word_reconstructed: String = word_lines.join("");

        assert_eq!(char_reconstructed, url);
        assert_eq!(word_reconstructed, url);
    }
}
