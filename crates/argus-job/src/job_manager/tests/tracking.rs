use super::*;

#[tokio::test]
async fn tracked_job_status_moves_from_pending_to_completed_to_consumed() {
    let manager = test_job_manager();
    let thread_id = ThreadId::new();
    let result = sample_job_result("job-42");

    manager.record_dispatched_job(thread_id, result.job_id.clone());
    assert!(matches!(
        manager.get_job_result_status(thread_id, &result.job_id, false),
        JobLookup::Pending
    ));

    manager.record_completed_job_result(thread_id, result.clone());
    assert!(matches!(
        manager.get_job_result_status(thread_id, &result.job_id, false),
        JobLookup::Completed(found) if found.job_id == result.job_id
    ));

    assert!(matches!(
        manager.get_job_result_status(thread_id, &result.job_id, true),
        JobLookup::Completed(found) if found.job_id == result.job_id
    ));

    assert!(matches!(
        manager.get_job_result_status(thread_id, &result.job_id, false),
        JobLookup::Consumed(found) if found.job_id == result.job_id
    ));
}

#[tokio::test]
async fn tracked_job_store_prunes_oldest_terminal_results() {
    let manager = test_job_manager();
    let thread_id = ThreadId::new();

    for index in 0..1030 {
        let result = sample_job_result(format!("job-terminal-{index}"));
        manager.record_completed_job_result(thread_id, result);
    }

    assert!(matches!(
        manager.get_job_result_status(thread_id, "job-terminal-0", false),
        JobLookup::NotFound
    ));
    assert!(matches!(
        manager.get_job_result_status(thread_id, "job-terminal-1029", false),
        JobLookup::Completed(found) if found.job_id == "job-terminal-1029"
    ));
}

#[tokio::test]
async fn tracked_job_store_prunes_consumed_results_after_retention_window() {
    let manager = test_job_manager();
    let thread_id = ThreadId::new();
    let oldest = sample_job_result("job-consumed-oldest");

    manager.record_completed_job_result(thread_id, oldest.clone());
    assert!(matches!(
        manager.get_job_result_status(thread_id, &oldest.job_id, true),
        JobLookup::Completed(found) if found.job_id == oldest.job_id
    ));
    assert!(matches!(
        manager.get_job_result_status(thread_id, &oldest.job_id, false),
        JobLookup::Consumed(found) if found.job_id == oldest.job_id
    ));

    for index in 0..1030 {
        let result = sample_job_result(format!("job-consumed-fill-{index}"));
        manager.record_completed_job_result(thread_id, result);
    }

    assert!(matches!(
        manager.get_job_result_status(thread_id, &oldest.job_id, false),
        JobLookup::NotFound
    ));
}

#[tokio::test]
async fn tracked_job_store_never_prunes_pending_or_cancelling_entries() {
    let manager = test_job_manager();
    let thread_id = ThreadId::new();
    let pending_job_id = "job-pending-retained".to_string();
    let cancelling_job_id = "job-cancelling-retained".to_string();

    manager.record_dispatched_job(thread_id, pending_job_id.clone());
    manager.record_dispatched_job(thread_id, cancelling_job_id.clone());
    manager
        .stop_job(&cancelling_job_id)
        .expect("stop_job should move tracked state to cancelling");

    for index in 0..1030 {
        let result = sample_job_result(format!("job-retention-fill-{index}"));
        manager.record_completed_job_result(thread_id, result);
    }

    assert!(matches!(
        manager.get_job_result_status(thread_id, &pending_job_id, false),
        JobLookup::Pending
    ));
    assert!(matches!(
        manager.get_job_result_status(thread_id, &cancelling_job_id, false),
        JobLookup::Pending
    ));
}
