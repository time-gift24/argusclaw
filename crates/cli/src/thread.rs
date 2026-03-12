//! Thread command implementation (currently placeholder.
//!
//! The implementation:
1. Provider selection from configuration
2. System prompt and message history
3. Database migration for thread messages table
4. Interactive input handling

5. Thread continuation resumes existing threads and queries/reloads thread by ID
6. Delete threads
7. Thread commands
enum ThreadCommand {
    /// Start a new thread from a provider.
    Start {
        provider: String,
        #[arg(long)]
        system_prompt: String,
    },

    /// Continue an existing thread.
    Continue {
        /// Messages are persisted to thread.
        thread_id: String,
    },

    /// List all threads.
    List {
        /// Delete a thread by ID.
        thread_id: String,
    },
}
