pub const FG_RESET: &[u8] = b"\x1b[39m";

pub const EMPTY_MARKER: u32 = 0x1D640; // 𝙀
pub const EMPTY_MARKER_BYTES: &[u8] = b"\xf0\x9d\x99\x80";
pub const IMMUTABLE_MARKER: u32 = 0x1D644; // 𝙄
pub const IMMUTABLE_MARKER_BYTES: &[u8] = b"\xf0\x9d\x99\x84";

pub fn skip_csi(bytes: &[u8], i: usize) -> Option<usize> {
    if i + 1 >= bytes.len() || bytes[i] != 0x1b || bytes[i + 1] != b'[' {
        return None;
    }
    let mut j = i + 2;
    while j < bytes.len() {
        let b = bytes[j];
        j += 1;
        if (0x40..=0x7e).contains(&b) {
            return Some(j);
        }
    }
    Some(j)
}

pub fn decode_utf8(bytes: &[u8], i: usize) -> (u32, usize) {
    let b = bytes[i];
    if b < 0x80 {
        return (b as u32, 1);
    }
    if b < 0xc0 {
        return (b as u32, 1);
    }
    if b < 0xe0 && i + 1 < bytes.len() {
        let cp = ((b & 0x1f) as u32) << 6 | (bytes[i + 1] & 0x3f) as u32;
        return (cp, 2);
    }
    if b < 0xf0 && i + 2 < bytes.len() {
        let cp = ((b & 0x0f) as u32) << 12
            | ((bytes[i + 1] & 0x3f) as u32) << 6
            | (bytes[i + 2] & 0x3f) as u32;
        return (cp, 3);
    }
    if i + 3 < bytes.len() {
        let cp = ((b & 0x07) as u32) << 18
            | ((bytes[i + 1] & 0x3f) as u32) << 12
            | ((bytes[i + 2] & 0x3f) as u32) << 6
            | (bytes[i + 3] & 0x3f) as u32;
        return (cp, 4);
    }
    (b as u32, 1)
}

pub fn is_fg_color_sgr(params: &str) -> bool {
    let parts: Vec<&str> = params.split(';').collect();
    match parts
        .first()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(999)
    {
        30..=37 | 39 | 90..=97 => true,
        38 => parts
            .get(1)
            .map(|p| *p == "5" || *p == "2")
            .unwrap_or(false),
        _ => false,
    }
}

// Emit ANSI sequences from `bytes`, optionally filtering params.
// `filter` returns true for params we should DROP.
pub fn emit_filtered_ansi(bytes: &[u8], out: &mut Vec<u8>, filter: impl Fn(&str) -> bool) {
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = skip_csi(bytes, i) {
            if bytes[i] == 0x1b
                && i + 1 < bytes.len()
                && bytes[i + 1] == b'['
                && end > 0
                && bytes[end - 1] == b'm'
            {
                let params = std::str::from_utf8(&bytes[i + 2..end - 1]).unwrap_or("");
                if !filter(params) {
                    out.extend_from_slice(&bytes[i..end]);
                }
            } else {
                out.extend_from_slice(&bytes[i..end]);
            }
            i = end;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_csi_basic_sgr() {
        assert_eq!(skip_csi(b"\x1b[31mfoo", 0), Some(5));
    }

    #[test]
    fn skip_csi_no_escape_returns_none() {
        assert_eq!(skip_csi(b"foo", 0), None);
    }

    #[test]
    fn skip_csi_with_multi_param_sgr() {
        assert_eq!(skip_csi(b"\x1b[38;5;245mX", 0), Some(11));
    }

    #[test]
    fn skip_csi_unterminated_returns_buffer_end() {
        assert_eq!(skip_csi(b"\x1b[38;5", 0), Some(6));
    }

    #[test]
    fn skip_csi_at_offset() {
        assert_eq!(skip_csi(b"X\x1b[1m", 1), Some(5));
    }

    #[test]
    fn decode_utf8_ascii() {
        assert_eq!(decode_utf8(b"A", 0), (0x41, 1));
    }

    #[test]
    fn decode_utf8_two_byte() {
        assert_eq!(decode_utf8(b"\xc2\xa3", 0), (0xa3, 2));
    }

    #[test]
    fn decode_utf8_three_byte_circle() {
        assert_eq!(decode_utf8(b"\xe2\x97\x8b", 0), (0x25CB, 3));
    }

    #[test]
    fn decode_utf8_three_byte_diamond() {
        assert_eq!(decode_utf8(b"\xe2\x97\x86", 0), (0x25C6, 3));
    }

    #[test]
    fn decode_utf8_four_byte_empty_marker() {
        assert_eq!(decode_utf8(EMPTY_MARKER_BYTES, 0), (EMPTY_MARKER, 4));
    }

    #[test]
    fn decode_utf8_four_byte_immutable_marker() {
        assert_eq!(decode_utf8(IMMUTABLE_MARKER_BYTES, 0), (IMMUTABLE_MARKER, 4));
    }

    #[test]
    fn fg_color_basic_30s() {
        for code in 30u16..=37 {
            assert!(is_fg_color_sgr(&code.to_string()));
        }
        assert!(is_fg_color_sgr("39"));
    }

    #[test]
    fn fg_color_bright_90s() {
        for code in 90u16..=97 {
            assert!(is_fg_color_sgr(&code.to_string()));
        }
    }

    #[test]
    fn fg_color_extended_256_and_truecolor() {
        assert!(is_fg_color_sgr("38;5;245"));
        assert!(is_fg_color_sgr("38;2;255;199;83"));
    }

    #[test]
    fn fg_color_rejects_bg_and_attrs() {
        assert!(!is_fg_color_sgr("0"));
        assert!(!is_fg_color_sgr("1"));
        assert!(!is_fg_color_sgr("3"));
        assert!(!is_fg_color_sgr("40"));
        assert!(!is_fg_color_sgr("49"));
        assert!(!is_fg_color_sgr("48;5;1"));
    }

    #[test]
    fn fg_color_handles_empty_and_garbage() {
        assert!(!is_fg_color_sgr(""));
        assert!(!is_fg_color_sgr("xyz"));
    }

    #[test]
    fn filtered_ansi_drops_fg_keeps_attrs_and_text() {
        let mut out = Vec::new();
        emit_filtered_ansi(
            b"\x1b[1m\x1b[31mhello\x1b[39m\x1b[0m",
            &mut out,
            is_fg_color_sgr,
        );
        assert_eq!(out, b"\x1b[1mhello\x1b[0m");
    }

    #[test]
    fn filtered_ansi_passthrough_plain_text() {
        let mut out = Vec::new();
        emit_filtered_ansi(b"plain", &mut out, is_fg_color_sgr);
        assert_eq!(out, b"plain");
    }

    #[test]
    fn filtered_ansi_drops_truecolor_fg() {
        let mut out = Vec::new();
        emit_filtered_ansi(
            b"\x1b[38;2;255;199;83mtext\x1b[39m",
            &mut out,
            is_fg_color_sgr,
        );
        assert_eq!(out, b"text");
    }
}
