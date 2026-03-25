use argus_turn::TurnLogEvent;

#[test]
fn test_turn_start_serialization() {
    let event = TurnLogEvent::TurnStart {
        system_prompt: "You are helpful.".into(),
        model: "gpt-4o".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"turn_start\""));
    assert!(json.contains("You are helpful"));
}

#[test]
fn test_tool_result_with_error() {
    let event = TurnLogEvent::ToolResult {
        id: "call_1".into(),
        name: "bash".into(),
        result: "".into(),
        duration_ms: 100,
        error: Some("timeout".into()),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"error\":\"timeout\""));
}

#[test]
fn test_turn_end_serialization() {
    let event = TurnLogEvent::TurnEnd {
        token_usage: argus_protocol::TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
        },
        finish_reason: "stop".into(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"turn_end\""));
    assert!(json.contains("\"input_tokens\":100"));
}
