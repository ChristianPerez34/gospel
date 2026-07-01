pub(crate) fn truncate_text_bytes(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }

    let suffix = "\n\n[truncated]";
    let suffix_bytes = suffix.len();
    let allowed_bytes = max_bytes.saturating_sub(suffix_bytes);
    let mut last_boundary = 0;
    for (index, _) in text.char_indices() {
        if index > allowed_bytes {
            break;
        }
        last_boundary = index;
    }

    let truncated = text[..last_boundary].trim_end().to_string();
    (format!("{}{}", truncated, suffix), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_text_bytes_leaves_short_text_unchanged() {
        let (truncated, did_truncate) = truncate_text_bytes("hello", 5);

        assert!(!did_truncate);
        assert_eq!(truncated, "hello");
    }

    #[test]
    fn truncate_text_bytes_reserves_space_for_suffix() {
        let (truncated, did_truncate) = truncate_text_bytes("abcdefghijklmnopqrstuv", 20);

        assert!(did_truncate);
        assert_eq!(truncated.len(), 20);
        assert_eq!(truncated, "abcdefg\n\n[truncated]");
    }

    #[test]
    fn truncate_text_bytes_uses_utf8_boundaries() {
        let (truncated, did_truncate) = truncate_text_bytes("abécdefghijklmnop", 16);

        assert!(did_truncate);
        assert!(truncated.is_char_boundary(truncated.len()));
        assert_eq!(truncated, "ab\n\n[truncated]");
    }
}
