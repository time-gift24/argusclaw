use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tokenizers::models::bpe::BPE;
use tokenizers::pre_tokenizers::byte_level::ByteLevel;
use tokenizers::tokenizer::Tokenizer;

use crate::error::TokenizationError;

const GPT2_ASSET_DIR: &str = "assets/gpt2";
const GPT2_VOCAB_FILE: &str = "vocab.json";
const GPT2_MERGES_FILE: &str = "merges.txt";

/// Global tokenizer singleton.
///
/// Uses `OnceLock` so the GPT-2 BPE tokenizer is loaded exactly once.
/// If initialization fails (e.g. missing assets), the error is permanently cached —
/// subsequent calls will return the same error without retrying.
static TOKENIZER: OnceLock<Result<Tokenizer, TokenizationError>> = OnceLock::new();

/// Estimate token count for a string using the shared GPT-2 BPE tokenizer.
pub fn estimate_tokens(content: &str) -> Result<u32, TokenizationError> {
    count_text_tokens(content)
}

pub(crate) fn count_text_tokens(content: &str) -> Result<u32, TokenizationError> {
    let tokenizer = shared_tokenizer()?;
    let encoding =
        tokenizer
            .encode(content, false)
            .map_err(|err| TokenizationError::EncodeFailed {
                reason: err.to_string(),
            })?;

    u32::try_from(encoding.len()).map_err(|_| TokenizationError::CountOverflow {
        count: encoding.len(),
    })
}

pub(crate) fn count_total_tokens<'a, I>(contents: I) -> Result<u32, TokenizationError>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut total = 0u64;

    for content in contents {
        total += u64::from(count_text_tokens(content)?);
    }

    u32::try_from(total).map_err(|_| TokenizationError::CountOverflow {
        count: total.min(usize::MAX as u64) as usize,
    })
}

fn shared_tokenizer() -> Result<&'static Tokenizer, TokenizationError> {
    match TOKENIZER.get_or_init(build_default_tokenizer) {
        Ok(tokenizer) => Ok(tokenizer),
        Err(err) => Err(err.clone()),
    }
}

fn build_default_tokenizer() -> Result<Tokenizer, TokenizationError> {
    let (vocab_path, merges_path) = tokenizer_asset_paths();
    tracing::debug!(vocab = %vocab_path.display(), merges = %merges_path.display(), "loading BPE tokenizer assets");
    build_tokenizer_from_paths(&vocab_path, &merges_path)
}

fn tokenizer_asset_paths() -> (PathBuf, PathBuf) {
    let asset_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(GPT2_ASSET_DIR);
    (
        asset_dir.join(GPT2_VOCAB_FILE),
        asset_dir.join(GPT2_MERGES_FILE),
    )
}

fn build_tokenizer_from_paths(
    vocab_path: &Path,
    merges_path: &Path,
) -> Result<Tokenizer, TokenizationError> {
    ensure_asset_exists(vocab_path)?;
    ensure_asset_exists(merges_path)?;

    let vocab_path_str =
        path_to_utf8(vocab_path).map_err(|path| TokenizationError::InvalidAssetPath { path })?;
    let merges_path_str =
        path_to_utf8(merges_path).map_err(|path| TokenizationError::InvalidAssetPath { path })?;

    let bpe = BPE::from_file(vocab_path_str, merges_path_str)
        .build()
        .map_err(|err| TokenizationError::BuildFailed {
            vocab_path: vocab_path.to_path_buf(),
            merges_path: merges_path.to_path_buf(),
            reason: err.to_string(),
        })?;

    let mut tokenizer = Tokenizer::new(bpe);
    tokenizer.with_pre_tokenizer(Some(ByteLevel::default()));

    Ok(tokenizer)
}

fn ensure_asset_exists(path: &Path) -> Result<(), TokenizationError> {
    if path.exists() {
        Ok(())
    } else {
        Err(TokenizationError::AssetMissing {
            path: path.to_path_buf(),
        })
    }
}

fn path_to_utf8(path: &Path) -> Result<&str, PathBuf> {
    path.to_str().ok_or_else(|| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_text_tokens_loads_repo_assets() {
        let token_count = count_text_tokens("Hey there!").expect("tokenization should succeed");

        assert_eq!(token_count, 3);
    }

    #[test]
    fn count_text_tokens_returns_zero_for_empty_input() {
        let token_count = count_text_tokens("").expect("tokenization should succeed");

        assert_eq!(token_count, 0);
    }

    #[test]
    fn build_tokenizer_from_paths_returns_missing_asset_error_for_missing_vocab() {
        let err = build_tokenizer_from_paths(
            Path::new("/tmp/definitely-missing-vocab.json"),
            Path::new("/tmp/definitely-missing-merges.txt"),
        )
        .expect_err("missing assets should return an error");

        assert!(matches!(err, TokenizationError::AssetMissing { .. }));
    }
}
