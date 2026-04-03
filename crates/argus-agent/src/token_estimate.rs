use argus_protocol::llm::ChatMessage;

/// Heuristic token estimate for the current chat context.
pub(crate) fn estimate_token_count(messages: &[ChatMessage]) -> u32 {
    messages
        .iter()
        .map(|message| message.content.split_whitespace().count() as u32)
        .sum()
}
