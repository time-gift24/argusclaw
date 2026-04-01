#![allow(deprecated)]

use crate::error::TokenizationError;

fn push_token_count(total: &mut u64, add: u64) -> Result<(), TokenizationError> {
    *total = total
        .checked_add(add)
        .ok_or(TokenizationError::CountOverflow { count: usize::MAX })?;
    Ok(())
}

fn is_ascii_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
    )
}

/// Estimate token count using a lightweight compatibility tokenizer.
///
/// This is intentionally approximate and exists to preserve public API
/// compatibility for callers that previously depended on local token
/// estimation between turns.
#[deprecated(
    note = "Token counts are approximate between turns and authoritative after LLM responses."
)]
pub fn estimate_tokens(content: &str) -> Result<u32, TokenizationError> {
    count_text_tokens(content)
}

pub(crate) fn count_text_tokens(content: &str) -> Result<u32, TokenizationError> {
    let mut total = 0u64;
    let mut chars = content.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }

        if is_cjk(ch) {
            push_token_count(&mut total, 1)?;
            continue;
        }

        if is_ascii_word_char(ch) {
            while let Some(next) = chars.peek() {
                if is_ascii_word_char(*next) {
                    chars.next();
                } else {
                    break;
                }
            }
            push_token_count(&mut total, 1)?;
            continue;
        }

        if ch.is_alphanumeric() {
            while let Some(next) = chars.peek() {
                if next.is_alphanumeric() && !is_cjk(*next) {
                    chars.next();
                } else {
                    break;
                }
            }
            push_token_count(&mut total, 1)?;
            continue;
        }

        push_token_count(&mut total, 1)?;
    }

    u32::try_from(total).map_err(|_| TokenizationError::CountOverflow {
        count: total.min(usize::MAX as u64) as usize,
    })
}

pub(crate) fn count_total_tokens<'a, I>(contents: I) -> Result<u32, TokenizationError>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut total = 0u64;

    for content in contents {
        push_token_count(&mut total, u64::from(count_text_tokens(content)?))?;
    }

    u32::try_from(total).map_err(|_| TokenizationError::CountOverflow {
        count: total.min(usize::MAX as u64) as usize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_text_tokens_matches_basic_samples() {
        assert_eq!(count_text_tokens("test").unwrap(), 1);
        assert_eq!(count_text_tokens("test test").unwrap(), 2);
        assert_eq!(count_text_tokens("Hey there!").unwrap(), 3);
        assert_eq!(count_text_tokens("").unwrap(), 0);
    }

    #[test]
    fn count_text_tokens_counts_cjk_characters_individually() {
        assert_eq!(count_text_tokens("你好世界").unwrap(), 4);
    }
}
