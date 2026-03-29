use argus_agent::estimate_tokens;

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
