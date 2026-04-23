use super::*;

#[tokio::test]
async fn recover_parent_then_children_keeps_persisted_job_id_authoritative() {
    let provider = Arc::new(CapturingProvider::new("done", Duration::from_millis(1), 8));
    let (manager, agent_id, parent_thread_id) = test_job_manager_with_provider(provider).await;
    let child_thread_id = ThreadId::new();
    let child_job_id = "job-cache-authority".to_string();
    let parent_base_dir = manager
        .trace_base_dir_for_thread(parent_thread_id)
        .await
        .expect("parent trace dir should exist");
    let parent_metadata = recover_thread_metadata(&parent_base_dir)
        .await
        .expect("parent metadata should recover");
    let child_base_dir = child_thread_base_dir(&parent_base_dir, child_thread_id);
    let child_snapshot = manager
        .template_manager
        .get(agent_id)
        .await
        .expect("template lookup should succeed")
        .expect("agent snapshot should exist");
    persist_thread_metadata(
        &child_base_dir,
        &ThreadTraceMetadata {
            thread_id: child_thread_id,
            kind: ThreadTraceKind::Job,
            root_session_id: parent_metadata.root_session_id,
            parent_thread_id: Some(parent_thread_id),
            job_id: Some(child_job_id.clone()),
            agent_snapshot: child_snapshot,
        },
    )
    .await
    .expect("child metadata should persist");

    let recovered_parent = manager
        .recover_parent_job_thread_id(&child_thread_id)
        .await
        .expect("parent recovery should succeed");
    assert_eq!(recovered_parent, Some(parent_thread_id));

    let children = manager
        .recover_child_jobs_for_thread(parent_thread_id)
        .await
        .expect("child recovery should succeed");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].thread_id, child_thread_id);
    assert_eq!(
        children[0].job_id, child_job_id,
        "cached child listings must preserve the persisted job id"
    );
}
