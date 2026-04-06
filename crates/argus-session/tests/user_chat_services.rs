//! Basic tests for server-facing user chat types.

use argus_session::{UserChatError, UserPrincipal};

#[test]
fn user_chat_not_found_message_is_generic() {
    assert_eq!(UserChatError::NotFound.to_string(), "resource not found");
}

#[test]
fn user_chat_agent_not_enabled_message_is_generic() {
    assert_eq!(
        UserChatError::AgentNotEnabled.to_string(),
        "agent is not enabled"
    );
}

#[test]
fn user_principal_identity_is_stable() {
    let alice = UserPrincipal {
        user_id: 1,
        account: "alice@example.com".to_string(),
        display_name: "Alice".to_string(),
    };
    let other_alice = UserPrincipal {
        user_id: 1,
        account: "alice@example.com".to_string(),
        display_name: "Alice".to_string(),
    };
    let bob = UserPrincipal {
        user_id: 2,
        account: "bob@example.com".to_string(),
        display_name: "Bob".to_string(),
    };

    assert_eq!(alice.user_id, other_alice.user_id);
    assert_ne!(alice.user_id, bob.user_id);
}
