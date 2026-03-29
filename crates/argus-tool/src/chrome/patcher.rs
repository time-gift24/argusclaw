use super::error::ChromeToolError;

const CDC_PREFIX: &[u8] = b"cdc_";
const CDC_MARKER_SPAN: usize = 18;

pub fn patch_cdc_tokens(mut bytes: Vec<u8>, fill: u8) -> Result<Vec<u8>, ChromeToolError> {
    let mut index = 0usize;
    while index + CDC_PREFIX.len() <= bytes.len() {
        if bytes[index..].starts_with(CDC_PREFIX) {
            let patch_len = CDC_PREFIX.len() + CDC_MARKER_SPAN;
            let end = bytes.len().min(index + patch_len);
            bytes[index..end].fill(fill);
            index = end;
            continue;
        }
        index += 1;
    }

    Ok(bytes)
}
