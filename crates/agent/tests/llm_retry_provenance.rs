use std::path::Path;

const UPSTREAM_URL: &str = "https://github.com/nearai/ironclaw";
const UPSTREAM_COMMIT: &str = "bcef04b82108222c9041e733de459130badd4cd7";
const UPSTREAM_LICENSE: &str = "MIT OR Apache-2.0";

#[test]
fn vendored_retry_file_includes_provenance_header() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let retry = std::fs::read_to_string(manifest_dir.join("src/llm/retry.rs"))
        .expect("retry.rs should be readable");

    assert!(retry.contains(UPSTREAM_URL));
    assert!(retry.contains(UPSTREAM_COMMIT));
    assert!(retry.contains(UPSTREAM_LICENSE));
}

#[test]
fn third_party_notice_mentions_retry_file() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let notice = std::fs::read_to_string(manifest_dir.join("../../THIRD_PARTY_NOTICES.md"))
        .expect("THIRD_PARTY_NOTICES.md should be readable");

    assert!(notice.contains("crates/agent/src/llm/retry.rs"));
}
