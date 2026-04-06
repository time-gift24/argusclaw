//! Integration tests for UserChatServices.
//!
//! These tests verify:
//! - Error types produce meaningful messages
//! - UserPrincipal identity works for ownership checks

use argus_session::user_chat_services::{UserChatError, UserPrincipal};

fn alice() -> UserPrincipal {
    UserPrincipal {
        user_id: 1,
        account: "alice@example.com".to_string(),
        display_name: "Alice".to_string(),
    }
}

fn bob() -> UserPrincipal {
    UserPrincipal {
        user_id: 2,
        account: "bob@example.com".to_string(),
        display_name: "Bob".to_string(),
    }
}

/// Verify that UserChatError variants serialize meaningful messages.
#[test]
fn user_chat_error_session_not_found_message() {
    let session_id = argus_protocol::SessionId::new();
    let err = UserChatError::SessionNotFound {
        user_id: 42,
        session_id: session_id.clone(),
    };
    let msg = err.to_string();
    assert!(msg.contains(&session_id.to_string()), "error should contain session id");
    assert!(msg.contains("42"), "error should contain user id");
}

/// Verify that UserChatError ThreadNotFound variant works.
#[test]
fn user_chat_error_thread_not_found_message() {
    let thread_id = argus_protocol::ThreadId::new();
    let err = UserChatError::ThreadNotFound {
        user_id: 99,
        thread_id: thread_id.clone(),
    };
    let msg = err.to_string();
    assert!(msg.contains(&thread_id.to_string()), "error should contain thread id");
}

/// Verify that UserChatError AgentNotEnabled variant works.
#[test]
fn user_chat_error_agent_not_enabled_message() {
    let err = UserChatError::AgentNotEnabled {
        agent_id: argus_protocol::AgentId::new(7),
    };
    let msg = err.to_string();
    assert!(msg.contains("7"), "error should contain agent id");
    assert!(msg.contains("not enabled"), "error should describe the problem");
}

/// Verify UserPrincipal equality works for ownership checks.
#[test]
fn user_principal_identity() {
    let a1 = alice();
    let a2 = alice();
    assert_eq!(a1.user_id, a2.user_id);
    assert_eq!(a1.account, a2.account);

    let b = bob();
    assert_ne!(a1.user_id, b.user_id);
}
