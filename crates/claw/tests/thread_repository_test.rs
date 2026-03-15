//! Integration tests for SqliteThreadRepository.

use claw::ThreadId;
use claw::db::sqlite::{SqliteThreadRepository, connect, migrate};
use claw::db::thread::{MessageRecord, ThreadRecord, ThreadRepository};

async fn setup_test_db() -> SqliteThreadRepository {
    let pool = connect("sqlite::memory:").await.unwrap();
    migrate(&pool).await.unwrap();
    SqliteThreadRepository::new(pool)
}

fn create_test_thread(id: &ThreadId, provider_id: &str) -> ThreadRecord {
    ThreadRecord {
        id: *id,
        provider_id: provider_id.to_string(),
        title: Some("Test Thread".to_string()),
        token_count: 0,
        turn_count: 0,
        created_at: "2024-01-01 00:00:00".to_string(),
        updated_at: "2024-01-01 00:00:00".to_string(),
    }
}

fn create_test_message(thread_id: &ThreadId, seq: u32, role: &str, content: &str) -> MessageRecord {
    MessageRecord {
        id: None,
        thread_id: *thread_id,
        seq,
        role: role.to_string(),
        content: content.to_string(),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        created_at: "2024-01-01 00:00:00".to_string(),
    }
}

#[tokio::test]
async fn upsert_and_get_thread() {
    let repo = setup_test_db().await;
    let thread_id = ThreadId::new();
    let record = create_test_thread(&thread_id, "provider-1");

    repo.upsert_thread(&record).await.unwrap();

    let retrieved = repo.get_thread(&thread_id).await.unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, thread_id);
    assert_eq!(retrieved.provider_id, "provider-1");
    assert_eq!(retrieved.title, Some("Test Thread".to_string()));
}

#[tokio::test]
async fn get_thread_returns_none_for_missing() {
    let repo = setup_test_db().await;
    let missing_id = ThreadId::new();

    let result = repo.get_thread(&missing_id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn upsert_updates_existing_thread() {
    let repo = setup_test_db().await;
    let thread_id = ThreadId::new();
    let mut record = create_test_thread(&thread_id, "provider-1");
    repo.upsert_thread(&record).await.unwrap();

    // Update the thread
    record.title = Some("Updated Title".to_string());
    record.token_count = 100;
    record.turn_count = 2;
    record.updated_at = "2024-01-02 00:00:00".to_string();
    repo.upsert_thread(&record).await.unwrap();

    let retrieved = repo.get_thread(&thread_id).await.unwrap().unwrap();
    assert_eq!(retrieved.title, Some("Updated Title".to_string()));
    assert_eq!(retrieved.token_count, 100);
    assert_eq!(retrieved.turn_count, 2);
}

#[tokio::test]
async fn list_threads_returns_most_recent_first() {
    let repo = setup_test_db().await;

    // Create threads with different timestamps
    let id1 = ThreadId::new();
    let mut thread1 = create_test_thread(&id1, "provider-1");
    thread1.updated_at = "2024-01-01 00:00:00".to_string();
    repo.upsert_thread(&thread1).await.unwrap();

    let id2 = ThreadId::new();
    let mut thread2 = create_test_thread(&id2, "provider-1");
    thread2.updated_at = "2024-01-02 00:00:00".to_string();
    repo.upsert_thread(&thread2).await.unwrap();

    let id3 = ThreadId::new();
    let mut thread3 = create_test_thread(&id3, "provider-1");
    thread3.updated_at = "2024-01-03 00:00:00".to_string();
    repo.upsert_thread(&thread3).await.unwrap();

    let threads = repo.list_threads(10).await.unwrap();
    assert_eq!(threads.len(), 3);

    // Most recent first
    assert_eq!(threads[0].id, id3);
    assert_eq!(threads[1].id, id2);
    assert_eq!(threads[2].id, id1);
}

#[tokio::test]
async fn list_threads_respects_limit() {
    let repo = setup_test_db().await;

    for _ in 0..5 {
        let id = ThreadId::new();
        let thread = create_test_thread(&id, "provider-1");
        repo.upsert_thread(&thread).await.unwrap();
        // Small delay to ensure different timestamps
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    let threads = repo.list_threads(3).await.unwrap();
    assert_eq!(threads.len(), 3);
}

#[tokio::test]
async fn delete_thread_removes_thread_and_messages() {
    let repo = setup_test_db().await;
    let thread_id = ThreadId::new();
    let thread = create_test_thread(&thread_id, "provider-1");
    repo.upsert_thread(&thread).await.unwrap();

    // Add a message
    let message = create_test_message(&thread_id, 0, "user", "Hello");
    repo.add_message(&message).await.unwrap();

    // Delete the thread
    let deleted = repo.delete_thread(&thread_id).await.unwrap();
    assert!(deleted);

    // Thread should be gone
    let result = repo.get_thread(&thread_id).await.unwrap();
    assert!(result.is_none());

    // Messages should be gone too (CASCADE)
    let messages = repo.get_messages(&thread_id).await.unwrap();
    assert!(messages.is_empty());
}

#[tokio::test]
async fn delete_thread_returns_false_for_missing() {
    let repo = setup_test_db().await;
    let missing_id = ThreadId::new();

    let deleted = repo.delete_thread(&missing_id).await.unwrap();
    assert!(!deleted);
}

#[tokio::test]
async fn add_and_get_messages() {
    let repo = setup_test_db().await;
    let thread_id = ThreadId::new();
    let thread = create_test_thread(&thread_id, "provider-1");
    repo.upsert_thread(&thread).await.unwrap();

    // Add messages
    let msg1 = create_test_message(&thread_id, 0, "system", "You are helpful.");
    let msg2 = create_test_message(&thread_id, 1, "user", "Hello");
    let msg3 = create_test_message(&thread_id, 2, "assistant", "Hi there!");

    repo.add_message(&msg1).await.unwrap();
    repo.add_message(&msg2).await.unwrap();
    repo.add_message(&msg3).await.unwrap();

    let messages = repo.get_messages(&thread_id).await.unwrap();
    assert_eq!(messages.len(), 3);

    assert_eq!(messages[0].role, "system");
    assert_eq!(messages[0].content, "You are helpful.");
    assert_eq!(messages[1].role, "user");
    assert_eq!(messages[2].role, "assistant");
}

#[tokio::test]
async fn get_messages_returns_empty_for_missing_thread() {
    let repo = setup_test_db().await;
    let missing_id = ThreadId::new();

    let messages = repo.get_messages(&missing_id).await.unwrap();
    assert!(messages.is_empty());
}

#[tokio::test]
async fn get_recent_messages() {
    let repo = setup_test_db().await;
    let thread_id = ThreadId::new();
    let thread = create_test_thread(&thread_id, "provider-1");
    repo.upsert_thread(&thread).await.unwrap();

    // Add 5 messages
    for i in 0..5 {
        let msg = create_test_message(&thread_id, i, "user", &format!("Message {}", i));
        repo.add_message(&msg).await.unwrap();
    }

    // Get last 3 messages
    let messages = repo.get_recent_messages(&thread_id, 3).await.unwrap();
    assert_eq!(messages.len(), 3);

    // Should be in chronological order (oldest first among the recent ones)
    assert_eq!(messages[0].content, "Message 2");
    assert_eq!(messages[1].content, "Message 3");
    assert_eq!(messages[2].content, "Message 4");
}

#[tokio::test]
async fn delete_messages_before() {
    let repo = setup_test_db().await;
    let thread_id = ThreadId::new();
    let thread = create_test_thread(&thread_id, "provider-1");
    repo.upsert_thread(&thread).await.unwrap();

    // Add 5 messages
    for i in 0..5 {
        let msg = create_test_message(&thread_id, i, "user", &format!("Message {}", i));
        repo.add_message(&msg).await.unwrap();
    }

    // Delete messages with seq < 3
    let deleted = repo.delete_messages_before(&thread_id, 3).await.unwrap();
    assert_eq!(deleted, 3);

    let messages = repo.get_messages(&thread_id).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "Message 3");
    assert_eq!(messages[1].content, "Message 4");
}

#[tokio::test]
async fn update_thread_stats() {
    let repo = setup_test_db().await;
    let thread_id = ThreadId::new();
    let thread = create_test_thread(&thread_id, "provider-1");
    repo.upsert_thread(&thread).await.unwrap();

    repo.update_thread_stats(&thread_id, 500, 3).await.unwrap();

    let retrieved = repo.get_thread(&thread_id).await.unwrap().unwrap();
    assert_eq!(retrieved.token_count, 500);
    assert_eq!(retrieved.turn_count, 3);
}

#[tokio::test]
async fn message_with_tool_info() {
    let repo = setup_test_db().await;
    let thread_id = ThreadId::new();
    let thread = create_test_thread(&thread_id, "provider-1");
    repo.upsert_thread(&thread).await.unwrap();

    // Add assistant message with tool calls
    let msg = MessageRecord {
        id: None,
        thread_id,
        seq: 0,
        role: "assistant".to_string(),
        content: "".to_string(),
        tool_call_id: None,
        tool_name: None,
        tool_calls: Some(
            r#"[{"id":"call_123","function":{"name":"get_weather","arguments":"{}"}}]"#.to_string(),
        ),
        created_at: "2024-01-01 00:00:00".to_string(),
    };
    repo.add_message(&msg).await.unwrap();

    // Add tool result message
    let tool_msg = MessageRecord {
        id: None,
        thread_id,
        seq: 1,
        role: "tool".to_string(),
        content: r#"{"temp": 72}"#.to_string(),
        tool_call_id: Some("call_123".to_string()),
        tool_name: Some("get_weather".to_string()),
        tool_calls: None,
        created_at: "2024-01-01 00:00:00".to_string(),
    };
    repo.add_message(&tool_msg).await.unwrap();

    let messages = repo.get_messages(&thread_id).await.unwrap();
    assert_eq!(messages.len(), 2);

    let assistant_msg = &messages[0];
    assert_eq!(assistant_msg.role, "assistant");
    assert!(assistant_msg.tool_calls.is_some());

    let tool_result = &messages[1];
    assert_eq!(tool_result.role, "tool");
    assert_eq!(tool_result.tool_call_id, Some("call_123".to_string()));
    assert_eq!(tool_result.tool_name, Some("get_weather".to_string()));
}
