//! Thread-related Tauri commands.

use std::sync::Arc;

use tauri::State;

use crate::tauri_context::{ChatMessageData, TauriContext};

/// Get the default thread ID.
///
/// Returns the default thread ID that was created during initialization.
/// The frontend should use this ID for the default conversation.
#[tauri::command]
pub fn get_default_thread_id(tauri_ctx: State<'_, Arc<TauriContext>>) -> String {
    tauri_ctx.default_thread_id().to_string()
}

/// Subscribe to thread events.
///
/// Starts streaming events from the specified thread to the frontend
/// via Tauri events ("thread:event").
#[tauri::command]
pub async fn subscribe_thread(
    tauri_ctx: State<'_, Arc<TauriContext>>,
    thread_id: String,
) -> Result<(), String> {
    tauri_ctx.subscribe_thread(&thread_id).await
}

/// Send a message to a thread.
///
/// This is non-blocking - returns immediately and the response
/// comes through the event stream.
#[tauri::command]
pub async fn send_message(
    tauri_ctx: State<'_, Arc<TauriContext>>,
    thread_id: String,
    message: String,
) -> Result<(), String> {
    tauri_ctx.send_message(&thread_id, message).await
}

/// Get messages from a thread.
#[tauri::command]
pub async fn get_thread_messages(
    tauri_ctx: State<'_, Arc<TauriContext>>,
    thread_id: String,
) -> Result<Vec<ChatMessageData>, String> {
    tauri_ctx.get_messages(&thread_id).await
}

/// Create a new thread with a specific ID.
#[tauri::command]
pub async fn create_thread(
    tauri_ctx: State<'_, Arc<TauriContext>>,
    thread_id: String,
) -> Result<(), String> {
    tauri_ctx.create_thread(&thread_id).await
}
