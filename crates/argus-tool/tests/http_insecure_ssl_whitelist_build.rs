#[path = "../build_support/http_insecure_ssl_whitelist.rs"]
mod http_insecure_ssl_whitelist;

use http_insecure_ssl_whitelist::parse_insecure_ssl_suffixes;

#[test]
fn parse_insecure_ssl_suffixes_accepts_comments_and_blank_lines() {
    let suffixes = parse_insecure_ssl_suffixes(
        r#"
        # internal domains
        corp.local

        staging.example.internal
        "#,
    )
    .expect("expected suffixes to parse");

    assert_eq!(
        suffixes,
        vec![
            "corp.local".to_string(),
            "staging.example.internal".to_string()
        ]
    );
}

#[test]
fn parse_insecure_ssl_suffixes_rejects_wildcards() {
    let err = parse_insecure_ssl_suffixes("*.corp.local").expect_err("wildcard must be rejected");
    assert!(err.contains("*"));
}

#[test]
fn parse_insecure_ssl_suffixes_rejects_ip_addresses() {
    let err = parse_insecure_ssl_suffixes("10.0.0.1").expect_err("ip must be rejected");
    assert!(err.contains("IP"));
}
