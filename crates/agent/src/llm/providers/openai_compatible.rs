use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, ContentPart, FinishReason, LlmError,
    LlmEventStream, LlmProvider, LlmStreamEvent, ProviderCapabilities, RetryConfig, RetryProvider,
    Role, ThinkingConfig, ToolCall, ToolCallDelta, ToolCompletionRequest, ToolCompletionResponse,
    ToolDefinition,
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout: Duration,
    pub extra_headers: HashMap<String, String>,
}

impl OpenAiCompatibleConfig {
    #[must_use]
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            timeout: Duration::from_secs(60),
            extra_headers: HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_extra_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.insert(name.into(), value.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleFactoryConfig {
    pub provider: OpenAiCompatibleConfig,
    pub retry: Option<RetryConfig>,
}

impl OpenAiCompatibleFactoryConfig {
    #[must_use]
    pub fn new(provider: OpenAiCompatibleConfig) -> Self {
        Self {
            provider,
            retry: None,
        }
    }

    #[must_use]
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.retry = Some(retry);
        self
    }
}

pub fn create_openai_compatible_provider(
    config: OpenAiCompatibleFactoryConfig,
) -> Result<Arc<dyn LlmProvider>, LlmError> {
    let provider: Arc<dyn LlmProvider> = Arc::new(OpenAiCompatibleProvider::new(config.provider)?);
    if let Some(retry) = config.retry {
        Ok(Arc::new(RetryProvider::new(provider, retry)))
    } else {
        Ok(provider)
    }
}

pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: OpenAiCompatibleConfig) -> Result<Self, LlmError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let auth = HeaderValue::from_str(&format!("Bearer {}", config.api_key)).map_err(|e| {
            LlmError::RequestFailed {
                provider: "openai-compatible".to_string(),
                reason: format!("invalid authorization header: {e}"),
            }
        })?;
        headers.insert(AUTHORIZATION, auth);

        for (name, value) in &config.extra_headers {
            let header_name =
                HeaderName::from_bytes(name.as_bytes()).map_err(|e| LlmError::RequestFailed {
                    provider: "openai-compatible".to_string(),
                    reason: format!("invalid header name `{name}`: {e}"),
                })?;
            let header_value =
                HeaderValue::from_str(value).map_err(|e| LlmError::RequestFailed {
                    provider: "openai-compatible".to_string(),
                    reason: format!("invalid header value for `{name}`: {e}"),
                })?;
            headers.insert(header_name, header_value);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(config.timeout)
            .build()
            .map_err(|e| LlmError::RequestFailed {
                provider: "openai-compatible".to_string(),
                reason: format!("failed to build HTTP client: {e}"),
            })?;

        Ok(Self {
            client,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: config.model,
        })
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    async fn send_chat_request(
        &self,
        body: &ChatCompletionsRequest,
    ) -> Result<reqwest::Response, LlmError> {
        let response = self
            .client
            .post(self.endpoint())
            .json(body)
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed {
                provider: "openai-compatible".to_string(),
                reason: e.to_string(),
            })?;

        if response.status().is_success() {
            return Ok(response);
        }

        Err(map_http_error(response).await)
    }
}

fn model_supports_thinking(model: &str) -> bool {
    let model = model.to_ascii_lowercase();

    model.starts_with("glm-5")
        || model.starts_with("glm-4.7")
        || model.starts_with("glm-4.6")
        || model.starts_with("glm-4.5")
        || model.starts_with("glm-4.1v-thinking")
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            thinking: model_supports_thinking(&self.model),
        }
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let body = ChatCompletionsRequest::from_completion_request(&self.model, request, false);
        let response = self.send_chat_request(&body).await?;
        let payload: ChatCompletionResponse =
            response
                .json()
                .await
                .map_err(|e| LlmError::InvalidResponse {
                    provider: "openai-compatible".to_string(),
                    reason: e.to_string(),
                })?;

        let choice =
            payload
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| LlmError::InvalidResponse {
                    provider: "openai-compatible".to_string(),
                    reason: "response had no choices".to_string(),
                })?;
        let usage = payload.usage.unwrap_or_default();

        Ok(CompletionResponse {
            content: choice.message.content.unwrap_or_default(),
            reasoning_content: choice.message.reasoning_content,
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            finish_reason: parse_finish_reason(choice.finish_reason.as_deref()),
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let body =
            ChatCompletionsRequest::from_tool_completion_request(&self.model, request, false);
        let response = self.send_chat_request(&body).await?;
        let payload: ChatCompletionResponse =
            response
                .json()
                .await
                .map_err(|e| LlmError::InvalidResponse {
                    provider: "openai-compatible".to_string(),
                    reason: e.to_string(),
                })?;

        let choice =
            payload
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| LlmError::InvalidResponse {
                    provider: "openai-compatible".to_string(),
                    reason: "response had no choices".to_string(),
                })?;
        let usage = payload.usage.unwrap_or_default();

        Ok(ToolCompletionResponse {
            content: choice.message.content,
            reasoning_content: choice.message.reasoning_content,
            tool_calls: choice
                .message
                .tool_calls
                .unwrap_or_default()
                .into_iter()
                .map(ToolCall::try_from)
                .collect::<Result<Vec<_>, _>>()?,
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            finish_reason: parse_finish_reason(choice.finish_reason.as_deref()),
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn stream_complete(
        &self,
        request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let body = ChatCompletionsRequest::from_completion_request(&self.model, request, true);
        let response = self.send_chat_request(&body).await?;
        let stream = response
            .bytes_stream()
            .eventsource()
            .map(|event| match event {
                Ok(event) => parse_stream_frame(&event.data),
                Err(err) => vec![Err(LlmError::RequestFailed {
                    provider: "openai-compatible".to_string(),
                    reason: err.to_string(),
                })],
            })
            .flat_map(futures_util::stream::iter);

        Ok(Box::pin(stream))
    }

    async fn stream_complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let body = ChatCompletionsRequest::from_tool_completion_request(&self.model, request, true);
        let response = self.send_chat_request(&body).await?;
        let stream = response
            .bytes_stream()
            .eventsource()
            .map(|event| match event {
                Ok(event) => parse_stream_frame(&event.data),
                Err(err) => vec![Err(LlmError::RequestFailed {
                    provider: "openai-compatible".to_string(),
                    reason: err.to_string(),
                })],
            })
            .flat_map(futures_util::stream::iter);

        Ok(Box::pin(stream))
    }
}

#[derive(Debug, Serialize)]
struct ChatCompletionsRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

impl ChatCompletionsRequest {
    fn from_completion_request(model: &str, request: CompletionRequest, stream: bool) -> Self {
        Self {
            model: request.model.unwrap_or_else(|| model.to_string()),
            messages: request
                .messages
                .into_iter()
                .map(OpenAiMessage::from)
                .collect(),
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            stop: request.stop_sequences,
            thinking: request.thinking,
            tools: None,
            tool_choice: None,
            stream,
            stream_options: stream.then_some(StreamOptions {
                include_usage: true,
            }),
        }
    }

    fn from_tool_completion_request(
        model: &str,
        request: ToolCompletionRequest,
        stream: bool,
    ) -> Self {
        Self {
            model: request.model.unwrap_or_else(|| model.to_string()),
            messages: request
                .messages
                .into_iter()
                .map(OpenAiMessage::from)
                .collect(),
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            stop: None,
            thinking: request.thinking,
            tools: Some(
                request
                    .tools
                    .into_iter()
                    .map(OpenAiToolDefinition::from)
                    .collect(),
            ),
            tool_choice: request.tool_choice,
            stream,
            stream_options: stream.then_some(StreamOptions {
                include_usage: true,
            }),
        }
    }
}

#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Debug, Serialize)]
struct OpenAiToolDefinition {
    #[serde(rename = "type")]
    kind: &'static str,
    function: OpenAiFunctionDefinition,
}

impl From<ToolDefinition> for OpenAiToolDefinition {
    fn from(value: ToolDefinition) -> Self {
        Self {
            kind: "function",
            function: OpenAiFunctionDefinition {
                name: value.name,
                description: value.description,
                parameters: value.parameters,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct OpenAiFunctionDefinition {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: Role,
    content: OpenAiContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

impl From<ChatMessage> for OpenAiMessage {
    fn from(value: ChatMessage) -> Self {
        let content = if value.content_parts.is_empty() {
            OpenAiContent::Text(value.content)
        } else {
            let mut parts = Vec::new();
            if !value.content.is_empty() {
                parts.push(ContentPart::Text {
                    text: value.content.clone(),
                });
            }
            parts.extend(value.content_parts);
            OpenAiContent::Parts(parts)
        };

        Self {
            role: value.role,
            content,
            tool_call_id: value.tool_call_id,
            name: value.name,
            tool_calls: value.tool_calls.map(|calls| {
                calls
                    .into_iter()
                    .map(|call| OpenAiToolCall {
                        id: call.id,
                        kind: "function".to_string(),
                        function: OpenAiFunctionCall {
                            name: call.name,
                            arguments: call.arguments.to_string(),
                        },
                    })
                    .collect()
            }),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum OpenAiContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

impl TryFrom<OpenAiToolCall> for ToolCall {
    type Error = LlmError;

    fn try_from(value: OpenAiToolCall) -> Result<Self, Self::Error> {
        let arguments = serde_json::from_str(&value.function.arguments).map_err(|e| {
            LlmError::InvalidResponse {
                provider: "openai-compatible".to_string(),
                reason: format!("invalid tool call arguments: {e}"),
            }
        })?;

        Ok(Self {
            id: value.id,
            name: value.function.name,
            arguments,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionMessage {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Default, Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    #[serde(default)]
    choices: Vec<ChatCompletionChunkChoice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunkChoice {
    #[serde(default)]
    delta: ChatCompletionChunkDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatCompletionChunkDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ChatCompletionChunkToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunkToolCall {
    index: usize,
    id: Option<String>,
    function: Option<ChatCompletionChunkFunction>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunkFunction {
    name: Option<String>,
    arguments: Option<String>,
}

fn parse_stream_frame(data: &str) -> Vec<Result<crate::llm::LlmStreamEvent, LlmError>> {
    if data == "[DONE]" {
        return Vec::new();
    }

    let chunk: ChatCompletionChunk = match serde_json::from_str(data) {
        Ok(chunk) => chunk,
        Err(e) => {
            return vec![Err(LlmError::InvalidResponse {
                provider: "openai-compatible".to_string(),
                reason: format!("invalid SSE frame: {e}"),
            })];
        }
    };

    let mut events = Vec::new();

    if let Some(usage) = chunk.usage {
        events.push(Ok(LlmStreamEvent::Usage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
        }));
    }

    for choice in chunk.choices {
        if let Some(content) = choice.delta.content
            && !content.is_empty()
        {
            events.push(Ok(LlmStreamEvent::ContentDelta { delta: content }));
        }

        if let Some(reasoning_content) = choice.delta.reasoning_content
            && !reasoning_content.is_empty()
        {
            events.push(Ok(LlmStreamEvent::ReasoningDelta {
                delta: reasoning_content,
            }));
        }

        if let Some(tool_calls) = choice.delta.tool_calls {
            for tool_call in tool_calls {
                let function = tool_call.function;
                let name = function.as_ref().and_then(|item| item.name.clone());
                let arguments_delta = function.and_then(|item| item.arguments);
                events.push(Ok(LlmStreamEvent::ToolCallDelta(ToolCallDelta {
                    index: tool_call.index,
                    id: tool_call.id,
                    name,
                    arguments_delta,
                })));
            }
        }

        if let Some(finish_reason) = choice.finish_reason.as_deref() {
            events.push(Ok(LlmStreamEvent::Finished {
                finish_reason: parse_finish_reason(Some(finish_reason)),
            }));
        }
    }

    events
}

fn parse_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("stop") => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        Some("tool_calls") => FinishReason::ToolUse,
        Some("content_filter") => FinishReason::ContentFilter,
        _ => FinishReason::Unknown,
    }
}

async fn map_http_error(response: reqwest::Response) -> LlmError {
    let status = response.status();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs);
    let body = response.text().await.unwrap_or_default();

    match status.as_u16() {
        401 | 403 => LlmError::AuthFailed {
            provider: "openai-compatible".to_string(),
        },
        429 => LlmError::RateLimited {
            provider: "openai-compatible".to_string(),
            retry_after,
        },
        _ => LlmError::RequestFailed {
            provider: "openai-compatible".to_string(),
            reason: format!("HTTP {}: {}", status, body),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_completions_request_serializes_thinking() {
        let request = CompletionRequest::new(vec![ChatMessage::user("hi")])
            .with_thinking(ThinkingConfig::enabled().with_clear_thinking(false));

        let body = ChatCompletionsRequest::from_completion_request("glm-5", request, false);
        let json = serde_json::to_value(body).expect("request should serialize");

        assert_eq!(json["thinking"]["type"], "enabled");
        assert_eq!(json["thinking"]["clear_thinking"], false);
    }

    #[test]
    fn chat_completion_message_deserializes_reasoning_content() {
        let payload: ChatCompletionResponse = serde_json::from_str(
            r#"{
              "choices": [{
                "message": {
                  "content": "final answer",
                  "reasoning_content": "hidden chain"
                },
                "finish_reason": "stop"
              }],
              "usage": { "prompt_tokens": 3, "completion_tokens": 5 }
            }"#,
        )
        .expect("response should deserialize");

        assert_eq!(
            payload.choices[0].message.reasoning_content.as_deref(),
            Some("hidden chain")
        );
    }

    #[test]
    fn parse_stream_frame_extracts_reasoning_delta() {
        let events = parse_stream_frame(
            r#"{"choices":[{"delta":{"reasoning_content":"step 1"},"finish_reason":null}]}"#,
        )
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("reasoning frame should parse");

        assert_eq!(
            events,
            vec![LlmStreamEvent::ReasoningDelta {
                delta: "step 1".to_string(),
            }]
        );
    }

    #[test]
    fn parse_stream_frame_extracts_content_delta() {
        let events = parse_stream_frame(
            r#"{"choices":[{"delta":{"content":"hello"},"finish_reason":null}]}"#,
        )
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("content frame should parse");

        assert_eq!(
            events,
            vec![LlmStreamEvent::ContentDelta {
                delta: "hello".to_string(),
            }]
        );
    }

    #[test]
    fn parse_stream_frame_extracts_tool_call_delta() {
        let events = parse_stream_frame(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"search","arguments":"{\"q\":\"rus"}}]},"finish_reason":null}]}"#,
        )
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("tool call frame should parse");

        assert_eq!(
            events,
            vec![LlmStreamEvent::ToolCallDelta(ToolCallDelta {
                index: 0,
                id: Some("call_1".to_string()),
                name: Some("search".to_string()),
                arguments_delta: Some("{\"q\":\"rus".to_string()),
            })],
        );
    }

    #[test]
    fn parse_stream_frame_extracts_finish_and_usage() {
        let events = parse_stream_frame(
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#,
        )
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("finish frame should parse");

        assert_eq!(
            events,
            vec![
                LlmStreamEvent::Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                },
                LlmStreamEvent::Finished {
                    finish_reason: FinishReason::Stop,
                },
            ]
        );
    }

    #[test]
    fn parse_stream_frame_ignores_done_sentinel() {
        let events = parse_stream_frame("[DONE]");
        assert!(events.is_empty());
    }

    #[test]
    fn openai_compatible_provider_reports_thinking_for_glm5() {
        let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleConfig::new(
            "https://example.com/v1",
            "key",
            "glm-5",
        ))
        .expect("provider should build");

        assert!(provider.capabilities().thinking);
    }

    #[test]
    fn openai_compatible_provider_reports_no_thinking_for_legacy_model() {
        let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleConfig::new(
            "https://example.com/v1",
            "key",
            "gpt-4o-mini",
        ))
        .expect("provider should build");

        assert!(!provider.capabilities().thinking);
    }

    #[test]
    fn config_builder_applies_header() {
        let config = OpenAiCompatibleConfig::new("https://example.com/v1", "key", "model")
            .with_extra_header("x-test", "1");
        assert_eq!(config.extra_headers.get("x-test"), Some(&"1".to_string()));
    }

    #[test]
    fn factory_can_wrap_provider_with_retry() {
        let config = OpenAiCompatibleFactoryConfig::new(OpenAiCompatibleConfig::new(
            "https://example.com/v1",
            "key",
            "model",
        ))
        .with_retry(RetryConfig { max_retries: 2 });

        let provider =
            create_openai_compatible_provider(config).expect("factory should build provider");

        assert_eq!(provider.model_name(), "model");
    }
}
