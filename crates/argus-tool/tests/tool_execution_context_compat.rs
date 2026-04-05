use argus_protocol::ToolExecutionContext;
use argus_protocol::ids::ThreadId;

#[test]
fn tool_execution_context_supports_struct_literal_construction_without_control_sender() {
    let (pipe_tx, _) = tokio::sync::broadcast::channel(8);

    let _ctx = ToolExecutionContext {
        thread_id: ThreadId::new(),
        agent_id: None,
        pipe_tx,
    };
}
