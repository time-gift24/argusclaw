//! Thread core types.

/// Information about a Thread for listing and display.
#[derive(Debug, Clone)]
pub struct ThreadInfo {
    /// Thread ID.
    pub id: String,
    /// Number of messages in history.
    pub message_count: usize,
    /// Current token count.
    pub token_count: u32,
    /// Number of turns completed.
    pub turn_count: u32,
}

/// Thread state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThreadState {
    /// Thread is idle and ready to accept new messages.
    #[default]
    Idle,
    /// Thread is processing a Turn.
    Processing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_state_default_is_idle() {
        assert_eq!(ThreadState::default(), ThreadState::Idle);
    }

    #[test]
    fn thread_state_equality() {
        assert_eq!(ThreadState::Idle, ThreadState::Idle);
        assert_eq!(ThreadState::Processing, ThreadState::Processing);
        assert_ne!(ThreadState::Idle, ThreadState::Processing);
    }
}
