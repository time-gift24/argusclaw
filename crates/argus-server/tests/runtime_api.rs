mod support;

use axum::http::StatusCode;
use futures_util::StreamExt;
use tokio::time::{Duration, timeout};

use argus_server::routes::runtime::RuntimeStateResponse;

#[tokio::test]
async fn runtime_returns_snapshot_payload() {
    let ctx = support::TestContext::new().await;
    let response = ctx.get("/api/v1/runtime").await;

    assert_eq!(response.status(), StatusCode::OK);

    let body: RuntimeStateResponse = support::json_body(response).await;
    assert_eq!(body.thread_pool.snapshot.max_threads, 64);
    assert_eq!(body.thread_pool.runtimes.len(), 0);
    assert_eq!(body.job_runtime.snapshot.max_threads, 64);
    assert_eq!(body.job_runtime.runtimes.len(), 0);
}

#[tokio::test]
async fn runtime_events_streams_initial_snapshot_event() {
    let ctx = support::TestContext::new().await;
    let response = ctx.get("/api/v1/runtime/events").await;

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .expect("runtime events response should include content-type");
    assert!(
        content_type.starts_with("text/event-stream"),
        "unexpected content-type: {content_type}"
    );

    let mut body = response.into_body().into_data_stream();
    let chunk = timeout(Duration::from_secs(1), body.next())
        .await
        .expect("runtime events should yield the first event promptly")
        .expect("runtime events body should not end before first event")
        .expect("runtime events first chunk should be readable");
    let event = String::from_utf8(chunk.to_vec()).expect("sse chunk should be utf-8");

    assert!(event.contains("event: runtime.snapshot"));
    assert!(event.contains("\"thread_pool\""));
    assert!(event.contains("\"job_runtime\""));
}
