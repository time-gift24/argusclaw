use super::error::ChromeToolError;

const CDC_PREFIX: &[u8] = b"cdc_";
const CDC_MARKER_SPAN: usize = 18;

pub fn patch_cdc_tokens(mut bytes: Vec<u8>, fill: u8) -> Result<Vec<u8>, ChromeToolError> {
    let patch_len = CDC_PREFIX.len() + CDC_MARKER_SPAN;
    let mut index = 0usize;
    while index + CDC_PREFIX.len() <= bytes.len() {
        if bytes[index..].starts_with(CDC_PREFIX) && index + patch_len <= bytes.len() {
            bytes[index..index + patch_len].fill(fill);
            index += patch_len;
            continue;
        }
        index += 1;
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::patch_cdc_tokens;

    #[test]
    fn patcher_no_match_leaves_bytes_unchanged() {
        let input = b"plain-bytes-without-marker".to_vec();
        let output = patch_cdc_tokens(input.clone(), b'X').unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn patcher_patches_multiple_matches() {
        let input = b"cdc_111111111111111111xxcdc_222222222222222222yy".to_vec();
        let output = patch_cdc_tokens(input, b'Q').unwrap();

        let mut expected = Vec::new();
        expected.extend_from_slice(b"QQQQQQQQQQQQQQQQQQQQQQ");
        expected.extend_from_slice(b"xx");
        expected.extend_from_slice(b"QQQQQQQQQQQQQQQQQQQQQQ");
        expected.extend_from_slice(b"yy");
        assert_eq!(output, expected);
    }

    #[test]
    fn patcher_truncated_match_is_left_unchanged() {
        let input = b"prefix-cdc_123".to_vec();
        let output = patch_cdc_tokens(input.clone(), b'X').unwrap();
        assert_eq!(output, input);
    }
}
