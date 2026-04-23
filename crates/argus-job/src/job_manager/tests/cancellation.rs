use super::*;

#[tokio::test]
async fn stop_job_signals_cancellation_for_pending_job() {
    let manager = test_job_manager();
    let thread_id = ThreadId::new();
    let job_id = "job-stop-pending".to_string();

    manager.record_dispatched_job(thread_id, job_id.clone());

    assert!(matches!(
        manager.get_job_result_status(thread_id, &job_id, false),
        JobLookup::Pending
    ));

    manager
        .stop_job(&job_id)
        .expect("stop_job should succeed for pending job");
}

#[tokio::test]
async fn stop_job_allows_loading_runtime() {
    let manager = test_job_manager();
    let thread_id = ThreadId::new();
    let job_id = "job-stop-loading".to_string();

    manager.record_dispatched_job(thread_id, job_id.clone());
    manager.sync_job_runtime_metadata(thread_id, Some(job_id.clone()), None);
    manager.upsert_job_runtime_summary(
        thread_id,
        job_id.clone(),
        ThreadRuntimeStatus::Loading,
        0,
        Some(Utc::now().to_rfc3339()),
        true,
        None,
    );

    manager
        .stop_job(&job_id)
        .expect("stop_job should succeed while runtime is loading");
}

#[tokio::test]
async fn stop_job_returns_not_running_after_stop_already_requested() {
    let manager = test_job_manager();
    let thread_id = ThreadId::new();
    let job_id = "job-stop-repeat".to_string();

    manager.record_dispatched_job(thread_id, job_id.clone());
    manager
        .stop_job(&job_id)
        .expect("first stop_job should succeed");

    let error = manager
        .stop_job(&job_id)
        .expect_err("second stop_job should report not running");
    assert!(matches!(error, JobError::JobNotRunning(found) if found == job_id));
}

#[tokio::test]
async fn stop_job_returns_not_found_for_unknown_job() {
    let manager = test_job_manager();

    let result = manager.stop_job("nonexistent-job");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("job not found"));
}

#[tokio::test]
async fn stop_job_cancels_turn_and_broadcasts_cancelled_job_result() {
    let provider = Arc::new(CapturingProvider::new(
        "delayed reply",
        Duration::from_secs(5),
        24,
    ));
    let (manager, agent_id, originating_thread_id) = test_job_manager_with_provider(provider).await;
    let job_id = "job-stop-end-to-end".to_string();
    let (pipe_tx, mut pipe_rx) = broadcast::channel(32);

    manager
        .dispatch_job(
            originating_thread_id,
            job_id.clone(),
            agent_id,
            "please take your time".to_string(),
            None,
            pipe_tx,
        )
        .await
        .expect("dispatch should succeed");

    timeout(Duration::from_secs(5), async {
        loop {
            let status = manager
                .job_runtime_state()
                .runtimes
                .into_iter()
                .find(|runtime| runtime.job_id == job_id)
                .map(|runtime| runtime.status);
            if matches!(
                status,
                Some(ThreadRuntimeStatus::Queued | ThreadRuntimeStatus::Running)
            ) {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("job runtime should become active");

    manager
        .stop_job(&job_id)
        .expect("stop_job should succeed while job is active");

    let job_event = timeout(Duration::from_secs(5), async {
        loop {
            match pipe_rx.recv().await {
                Ok(ThreadEvent::JobResult {
                    thread_id,
                    job_id: event_job_id,
                    success,
                    cancelled,
                    message,
                    ..
                }) if event_job_id == job_id => {
                    assert_eq!(thread_id, originating_thread_id);
                    break (success, cancelled, message);
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("thread event channel should remain open");
                }
            }
        }
    })
    .await
    .expect("job result event should arrive");

    assert!(
        !job_event.0,
        "cancelled job should still report unsuccessful execution"
    );
    assert!(
        job_event.1,
        "cancelled job should report cancellation explicitly"
    );
    assert!(
        job_event.2.contains("Turn cancelled"),
        "unexpected cancel message: {}",
        job_event.2
    );

    assert!(matches!(
        manager.get_job_result_status(originating_thread_id, &job_id, false),
        JobLookup::Completed(ThreadJobResult {
            success: false,
            cancelled: true,
            ..
        })
    ));

    let persisted_job = manager
        .job_repository
        .as_ref()
        .expect("test manager should expose a job repository")
        .get(&argus_repository::types::JobId::new(job_id.clone()))
        .await
        .expect("cancelled job should persist")
        .expect("cancelled job record should exist");
    assert_eq!(persisted_job.status, JobStatus::Cancelled);

    let runtime = manager
        .job_runtime_state()
        .runtimes
        .into_iter()
        .find(|runtime| runtime.job_id == job_id)
        .expect("cancelled job runtime should remain tracked");
    assert_eq!(runtime.status, ThreadRuntimeStatus::Cooling);
    assert_eq!(runtime.last_reason, Some(ThreadPoolEventReason::Cancelled));
}
