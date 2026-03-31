#![allow(deprecated)]

use argus_agent::{Compactor, KeepTokensCompactor, TokenizationError, estimate_tokens};

#[test]
fn estimate_tokens_returns_zero_for_empty_input() {
    let token_count = estimate_tokens("").expect("tokenization should succeed");

    assert_eq!(token_count, 0);
}

#[test]
fn estimate_tokens_matches_expected_counts_for_basic_samples() {
    let samples = [("test", 1), ("test test", 2), ("Hey there!", 3)];

    for (input, expected) in samples {
        let token_count = estimate_tokens(input).expect("tokenization should succeed");
        assert_eq!(
            token_count, expected,
            "unexpected token count for {input:?}"
        );
    }
}

#[test]
fn keep_tokens_compactor_is_still_constructible_for_compatibility() {
    let compactor = KeepTokensCompactor::new(0.8, 0.5);

    assert_eq!(compactor.name(), "keep_tokens");
}

#[test]
fn tokenization_error_keeps_legacy_variants() {
    let err = TokenizationError::AssetMissing {
        path: "/tmp/missing.json".into(),
    };

    assert!(err.to_string().contains("missing.json"));
}
