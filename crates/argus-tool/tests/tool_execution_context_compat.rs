use argus_protocol::ToolExecutionContext;
use argus_protocol::ids::ThreadId;

#[test]
fn tool_execution_context_supports_legacy_struct_literal_construction() {
    let (pipe_tx, _) = tokio::sync::broadcast::channel(8);
    let (control_tx, _control_rx) = tokio::sync::mpsc::unbounded_channel();

    let _ctx = ToolExecutionContext {
        thread_id: ThreadId::new(),
        agent_id: None,
        pipe_tx,
        control_tx,
    };
}
