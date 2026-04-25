use super::*;

#[tokio::test]
async fn dispatch_job_creates_thread_pool_binding() {
    let manager = test_job_manager();
    let originating_thread_id = ThreadId::new();
    let (pipe_tx, _pipe_rx) = broadcast::channel(16);
    let job_id = "job-bound".to_string();

    manager
        .dispatch_job(
            originating_thread_id,
            job_id.clone(),
            AgentId::new(99),
            "run this".to_string(),
            None,
            pipe_tx,
        )
        .await
        .expect("job should enqueue even if execution later fails");

    let bound_thread_id = manager
        .thread_binding(&job_id)
        .expect("job should be bound to a thread");
    let runtime = manager
        .job_runtime_state()
        .runtimes
        .into_iter()
        .find(|runtime| runtime.thread_id == bound_thread_id)
        .expect("bound runtime should be tracked in job runtime state");
    assert_eq!(runtime.job_id, job_id);
    assert!(matches!(
        runtime.status,
        argus_protocol::ThreadRuntimeStatus::Queued
            | argus_protocol::ThreadRuntimeStatus::Running
            | argus_protocol::ThreadRuntimeStatus::Cooling
    ));
}

#[tokio::test]
async fn dispatch_job_notifies_child_thread_created_hook() {
    let manager = test_job_manager();
    let originating_thread_id = ThreadId::new();
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed_hook = Arc::clone(&observed);
    manager.set_job_thread_created_hook(Some(Arc::new(move |parent, child| {
        observed_hook
            .lock()
            .expect("observed hook mutex should not be poisoned")
            .push((parent, child));
    })));
    let (pipe_tx, _pipe_rx) = broadcast::channel(16);
    let job_id = "job-hook".to_string();

    manager
        .dispatch_job(
            originating_thread_id,
            job_id.clone(),
            AgentId::new(99),
            "run hook".to_string(),
            None,
            pipe_tx,
        )
        .await
        .expect("job should enqueue even if execution later fails");

    let bound_thread_id = manager
        .thread_binding(&job_id)
        .expect("job should be bound to a thread");
    assert_eq!(
        observed
            .lock()
            .expect("observed hook mutex should not be poisoned")
            .as_slice(),
        &[(originating_thread_id, bound_thread_id)]
    );
}

#[tokio::test]
async fn alpha_dispatch_job_emits_binding_queue_metrics_and_result_events() {
    let manager = test_job_manager();
    let originating_thread_id = ThreadId::new();
    let (pipe_tx, mut pipe_rx) = broadcast::channel(32);
    let job_id = "alpha-job-event-flow".to_string();

    manager
        .dispatch_job(
            originating_thread_id,
            job_id.clone(),
            AgentId::new(99),
            "run alpha event flow".to_string(),
            None,
            pipe_tx,
        )
        .await
        .expect("job should enqueue even if execution later fails");

    let mut bound_thread_id = None;
    let mut saw_queued = false;
    let mut saw_failure_update = false;
    let mut saw_metrics = false;
    let mut saw_result = false;

    timeout(Duration::from_secs(5), async {
        while !saw_result {
            match pipe_rx.recv().await {
                Ok(ThreadEvent::ThreadBoundToJob {
                    job_id: event_job_id,
                    thread_id: execution_thread_id,
                }) if event_job_id == job_id => {
                    assert_ne!(execution_thread_id, originating_thread_id);
                    bound_thread_id = Some(execution_thread_id);
                }
                Ok(ThreadEvent::JobRuntimeQueued {
                    thread_id,
                    job_id: event_job_id,
                }) if event_job_id == job_id => {
                    if let Some(execution_thread_id) = bound_thread_id {
                        assert_eq!(thread_id, execution_thread_id);
                    }
                    saw_queued = true;
                }
                Ok(ThreadEvent::JobRuntimeMetricsUpdated { .. }) => {
                    saw_metrics = true;
                }
                Ok(ThreadEvent::JobRuntimeUpdated { runtime })
                    if runtime.job_id == job_id
                        && runtime.status == ThreadRuntimeStatus::Inactive
                        && runtime.last_reason == Some(ThreadPoolEventReason::ExecutionFailed) =>
                {
                    saw_failure_update = true;
                }
                Ok(ThreadEvent::JobResult {
                    thread_id,
                    job_id: event_job_id,
                    success,
                    ..
                }) if event_job_id == job_id => {
                    assert_eq!(thread_id, originating_thread_id);
                    assert!(
                        !success,
                        "alpha flow should surface execution failure when the agent record is missing"
                    );
                    saw_result = true;
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

    let execution_thread_id = bound_thread_id.expect("job should bind to an execution thread");
    assert_eq!(manager.thread_binding(&job_id), Some(execution_thread_id));
    assert!(saw_queued, "queued event should be observed");
    assert!(
        saw_failure_update,
        "load failure should publish a runtime update"
    );
    assert!(saw_metrics, "metrics update should be observed");
}

#[tokio::test]
async fn cooling_job_eviction_publishes_job_runtime_events_through_parent_thread() {
    let provider = Arc::new(CapturingProvider::new(
        "done",
        Duration::from_millis(10),
        24,
    ));
    let (manager, agent_id, originating_thread_id) = test_job_manager_with_provider(provider).await;
    manager.thread_pool().register_runtime(
        originating_thread_id,
        ThreadRuntimeStatus::Inactive,
        0,
        None,
        true,
        None,
        None,
    );
    let mut parent_rx = manager
        .thread_pool()
        .subscribe(&originating_thread_id)
        .expect("parent runtime should be registered");
    let (pipe_tx, _pipe_rx) = broadcast::channel(32);
    let job_id = "job-eviction-bridge".to_string();

    manager
        .dispatch_job(
            originating_thread_id,
            job_id.clone(),
            agent_id,
            "finish quickly".to_string(),
            None,
            pipe_tx,
        )
        .await
        .expect("dispatch should succeed");

    let execution_thread_id = timeout(Duration::from_secs(5), async {
        loop {
            if let Some(thread_id) = manager.thread_binding(&job_id) {
                let status = manager
                    .job_runtime_summary(&thread_id)
                    .map(|runtime| runtime.status);
                if matches!(status, Some(ThreadRuntimeStatus::Cooling)) {
                    break thread_id;
                }
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("job runtime should cool after completion");

    manager
        .thread_pool()
        .evict_runtime(&execution_thread_id, ThreadPoolEventReason::CoolingExpired)
        .await
        .expect("cooling job runtime should be evictable");

    let mut saw_updated = false;
    let mut saw_evicted = false;
    timeout(Duration::from_secs(5), async {
        while !(saw_updated && saw_evicted) {
            match parent_rx.recv().await {
                Ok(ThreadEvent::JobRuntimeUpdated { runtime })
                    if runtime.job_id == job_id
                        && runtime.thread_id == execution_thread_id
                        && runtime.status == ThreadRuntimeStatus::Evicted
                        && runtime.last_reason == Some(ThreadPoolEventReason::CoolingExpired) =>
                {
                    saw_updated = true;
                }
                Ok(ThreadEvent::JobRuntimeEvicted {
                    thread_id,
                    job_id: event_job_id,
                    reason,
                }) if event_job_id == job_id
                    && thread_id == execution_thread_id
                    && reason == ThreadPoolEventReason::CoolingExpired =>
                {
                    saw_evicted = true;
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("parent thread event channel should remain open");
                }
            }
        }
    })
    .await
    .expect("parent thread should observe job runtime eviction");
}

#[tokio::test]
async fn dispatch_job_enqueue_failure_does_not_leave_pending_tracking() {
    let manager = test_persistent_job_manager_without_default_provider().await;
    let originating_thread_id = ThreadId::new();
    let (pipe_tx, _pipe_rx) = broadcast::channel(16);
    let job_id = "job-enqueue-failure".to_string();

    let dispatch_result = manager
        .dispatch_job(
            originating_thread_id,
            job_id.clone(),
            AgentId::new(999_999),
            "run this".to_string(),
            None,
            pipe_tx,
        )
        .await;

    assert!(dispatch_result.is_err());
    assert!(matches!(
        manager.get_job_result_status(originating_thread_id, &job_id, false),
        JobLookup::NotFound
    ));
}
