use std::path::Path;

const UPSTREAM_URL: &str = "https://github.com/nearai/ironclaw";
const UPSTREAM_COMMIT: &str = "bcef04b82108222c9041e733de459130badd4cd7";
const UPSTREAM_LICENSE: &str = "MIT OR Apache-2.0";

#[test]
fn vendored_llm_files_include_provenance_header() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let provider = std::fs::read_to_string(manifest_dir.join("src/llm/provider.rs"))
        .expect("provider.rs should be readable");
    let error = std::fs::read_to_string(manifest_dir.join("src/llm/error.rs"))
        .expect("error.rs should be readable");

    for contents in [&provider, &error] {
        assert!(contents.contains(UPSTREAM_URL));
        assert!(contents.contains(UPSTREAM_COMMIT));
        assert!(contents.contains(UPSTREAM_LICENSE));
    }
}

#[test]
fn third_party_notice_matches_vendored_files() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let notice = std::fs::read_to_string(manifest_dir.join("../../THIRD_PARTY_NOTICES.md"))
        .expect("THIRD_PARTY_NOTICES.md should be readable");

    assert!(notice.contains("nearai/ironclaw"));
    assert!(notice.contains(UPSTREAM_COMMIT));
    assert!(notice.contains("crates/agent/src/llm/provider.rs"));
    assert!(notice.contains("crates/agent/src/llm/error.rs"));
}
