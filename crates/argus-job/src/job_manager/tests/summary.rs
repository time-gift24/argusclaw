use super::*;

#[test]
fn summarize_output_handles_unicode_boundaries() {
    let content = format!("{}数{}", "a".repeat(498), "b".repeat(5000));

    let summary = JobManager::summarize_output(&assistant_output(&content));

    assert!(summary.ends_with("..."));
    assert_eq!(
        summary.chars().count(),
        JobManager::JOB_RESULT_SUMMARY_CHAR_LIMIT + 3
    );
    assert!(summary.contains('数'));
}

#[test]
fn summarize_output_keeps_reports_longer_than_legacy_limit() {
    let content = "x".repeat(800);

    let summary = JobManager::summarize_output(&assistant_output(&content));

    assert_eq!(summary, content);
}
