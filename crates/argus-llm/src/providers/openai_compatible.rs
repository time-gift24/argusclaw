use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use argus_protocol::llm::{
    CompletionRequest, CompletionResponse, ContentPart, LlmError, LlmEventStream, LlmProvider,
    LlmStreamEvent, ProviderCapabilities, Role, ThinkingConfig, ToolCall, ToolCallDelta,
    ToolDefinition,
};

use crate::retry::{RetryConfig, RetryProvider};

const DEFAULT_RAW_STREAM_TAIL_BYTES: usize = 64 * 1024;
pub(crate) const DEFAULT_OPENAI_COMPATIBLE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
struct RawStreamCapture {
    limit: usize,
    buffer: Vec<u8>,
}

impl RawStreamCapture {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            buffer: Vec::new(),
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > self.limit {
            let excess = self.buffer.len() - self.limit;
            self.buffer.drain(..excess);
        }
    }

    fn preview(&self) -> String {
        String::from_utf8_lossy(&self.buffer).into_owned()
    }

    fn len(&self) -> usize {
        self.buffer.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReqwestErrorDiagnostics {
    is_timeout: bool,
    is_connect: bool,
    is_body: bool,
    is_decode: bool,
    is_request: bool,
    is_builder: bool,
    is_status: bool,
    status_code: Option<u16>,
    url: Option<String>,
    source_chain: String,
}

fn error_source_chain(err: &(dyn Error + 'static)) -> String {
    let mut parts = vec![err.to_string()];
    let mut source = err.source();
    while let Some(next) = source {
        parts.push(next.to_string());
        source = next.source();
    }
    parts.join(" <- ")
}

fn reqwest_error_diagnostics(err: &reqwest::Error) -> ReqwestErrorDiagnostics {
    ReqwestErrorDiagnostics {
        is_timeout: err.is_timeout(),
        is_connect: err.is_connect(),
        is_body: err.is_body(),
        is_decode: err.is_decode(),
        is_request: err.is_request(),
        is_builder: err.is_builder(),
        is_status: err.is_status(),
        status_code: err.status().map(|status| status.as_u16()),
        url: err.url().map(ToString::to_string),
        source_chain: error_source_chain(err),
    }
}

fn log_stream_transport_error(
    stage: &str,
    err: &reqwest::Error,
    raw_stream_capture: &Arc<Mutex<RawStreamCapture>>,
) {
    let capture = raw_stream_capture
        .lock()
        .expect("raw stream capture mutex poisoned");
    let raw_stream_tail = capture.preview();
    let diagnostics = reqwest_error_diagnostics(err);

    tracing::error!(
        stage,
        error = %err,
        is_timeout = diagnostics.is_timeout,
        is_connect = diagnostics.is_connect,
        is_body = diagnostics.is_body,
        is_decode = diagnostics.is_decode,
        is_request = diagnostics.is_request,
        is_builder = diagnostics.is_builder,
        is_status = diagnostics.is_status,
        status_code = diagnostics.status_code,
        url = diagnostics.url.as_deref().unwrap_or(""),
        source_chain = %diagnostics.source_chain,
        raw_stream_tail_bytes = capture.len(),
        raw_stream_tail = %raw_stream_tail,
        "stream transport failed; dumping recent raw response bytes"
    );
}

fn build_sse_event_stream<S, B>(
    stage: &'static str,
    byte_stream: S,
) -> impl futures_util::Stream<Item = Result<LlmStreamEvent, LlmError>>
where
    S: futures_util::Stream<Item = Result<B, reqwest::Error>> + Send + 'static,
    B: AsRef<[u8]>,
{
    SseEventStream::new(
        stage,
        Box::pin(byte_stream.map(|chunk| chunk.map(|bytes| bytes.as_ref().to_vec()))),
    )
}

#[derive(Debug, Default)]
struct SseFrameDecoder {
    buffer: Vec<u8>,
}

impl SseFrameDecoder {
    fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, bytes: &[u8]) -> Vec<Result<LlmStreamEvent, LlmError>> {
        self.buffer.extend_from_slice(bytes);

        let mut events = Vec::new();
        while let Some((frame_end, separator_len)) = find_sse_frame_boundary(&self.buffer) {
            let frame = self.buffer[..frame_end].to_vec();
            self.buffer.drain(..frame_end + separator_len);

            if let Some(data) = extract_sse_data(&frame) {
                events.extend(parse_stream_frame(&data));
            }
        }

        events
    }

    fn finish(&mut self) -> Vec<Result<LlmStreamEvent, LlmError>> {
        if self.buffer.is_empty() {
            return Vec::new();
        }

        let frame = std::mem::take(&mut self.buffer);
        extract_sse_data(&frame)
            .map(|data| parse_final_stream_frame(&data))
            .unwrap_or_default()
    }
}

struct SseEventStream {
    stage: &'static str,
    inner_stream: Pin<Box<dyn Stream<Item = Result<Vec<u8>, reqwest::Error>> + Send>>,
    decoder: SseFrameDecoder,
    pending_events: VecDeque<Result<LlmStreamEvent, LlmError>>,
    raw_stream_capture: Arc<Mutex<RawStreamCapture>>,
    done: bool,
}

impl SseEventStream {
    fn new(
        stage: &'static str,
        inner_stream: Pin<Box<dyn Stream<Item = Result<Vec<u8>, reqwest::Error>> + Send>>,
    ) -> Self {
        Self {
            stage,
            inner_stream,
            decoder: SseFrameDecoder::new(),
            pending_events: VecDeque::new(),
            raw_stream_capture: Arc::new(Mutex::new(RawStreamCapture::new(
                DEFAULT_RAW_STREAM_TAIL_BYTES,
            ))),
            done: false,
        }
    }
}

impl Stream for SseEventStream {
    type Item = Result<LlmStreamEvent, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(event) = self.pending_events.pop_front() {
                return Poll::Ready(Some(event));
            }

            if self.done {
                return Poll::Ready(None);
            }

            match self.inner_stream.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    let parsed_events = self.decoder.finish();
                    self.pending_events.extend(parsed_events);
                    self.done = true;
                    continue;
                }
                Poll::Ready(Some(Ok(bytes))) => {
                    self.raw_stream_capture
                        .lock()
                        .expect("raw stream capture mutex poisoned")
                        .push(&bytes);
                    let parsed_events = self.decoder.push(&bytes);
                    self.pending_events.extend(parsed_events);
                }
                Poll::Ready(Some(Err(err))) => {
                    log_stream_transport_error(self.stage, &err, &self.raw_stream_capture);
                    self.done = true;
                    return Poll::Ready(Some(Err(LlmError::StreamInterrupted {
                        provider: "openai-compatible".to_string(),
                        reason: err.to_string(),
                    })));
                }
            }
        }
    }
}

fn find_sse_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let mut index = 0usize;
    while index < buffer.len() {
        if let Some(first_len) = line_ending_len_at(buffer, index) {
            let next_index = index + first_len;
            if let Some(second_len) = line_ending_len_at(buffer, next_index) {
                return Some((index, first_len + second_len));
            }
            index = next_index;
            continue;
        }

        index += 1;
    }

    None
}

fn line_ending_len_at(buffer: &[u8], index: usize) -> Option<usize> {
    match buffer.get(index) {
        Some(b'\n') => Some(1),
        Some(b'\r') if buffer.get(index + 1) == Some(&b'\n') => Some(2),
        Some(b'\r') => Some(1),
        _ => None,
    }
}

fn extract_sse_data(frame: &[u8]) -> Option<String> {
    let mut data_lines = Vec::new();
    let mut index = 0usize;
    while index <= frame.len() {
        let (line_end, separator_len) = match next_line_break(frame, index) {
            Some((line_end, separator_len)) => (line_end, separator_len),
            None => (frame.len(), 0),
        };
        let line = String::from_utf8_lossy(&frame[index..line_end]);
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        } else if line == "data" {
            data_lines.push(String::new());
        }

        if separator_len == 0 {
            break;
        }
        index = line_end + separator_len;
    }

    if data_lines.is_empty() {
        None
    } else {
        Some(data_lines.join("\n"))
    }
}

fn next_line_break(buffer: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut index = start;
    while index < buffer.len() {
        if let Some(separator_len) = line_ending_len_at(buffer, index) {
            return Some((index, separator_len));
        }
        index += 1;
    }

    None
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout: Duration,
    pub extra_headers: HashMap<String, String>,
    pub context_window: u32,
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
            timeout: DEFAULT_OPENAI_COMPATIBLE_TIMEOUT,
            extra_headers: HashMap::new(),
            context_window: 128_000,
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

    #[must_use]
    pub fn with_context_window(mut self, context_window: u32) -> Self {
        self.context_window = context_window;
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
    request_client: reqwest::Client,
    stream_client: reqwest::Client,
    base_url: String,
    model: String,
    context_window: u32,
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

        let request_client = build_http_client(&headers, Some(config.timeout), None, None)?;
        let stream_client =
            build_http_client(&headers, None, Some(config.timeout), Some(config.timeout))?;

        Ok(Self {
            request_client,
            stream_client,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: config.model,
            context_window: config.context_window,
        })
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    async fn send_chat_request(
        &self,
        body: &ChatCompletionsRequest,
        extra_headers: &[(String, String)],
    ) -> Result<reqwest::Response, LlmError> {
        let response = self
            .build_chat_request(body, extra_headers)
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed {
                provider: "openai-compatible".to_string(),
                reason: e.to_string(),
            })?;

        if response.status().is_success() {
            return Ok(response);
        }

        Err(map_http_error(response, &self.model).await)
    }

    fn build_chat_request(
        &self,
        body: &ChatCompletionsRequest,
        extra_headers: &[(String, String)],
    ) -> reqwest::RequestBuilder {
        match serde_json::to_string_pretty(body) {
            Ok(json) => eprintln!("[openai-compatible] outgoing request json:\n{json}"),
            Err(error) => {
                eprintln!("[openai-compatible] failed to serialize outgoing request json: {error}")
            }
        }

        let client = if body.stream {
            &self.stream_client
        } else {
            &self.request_client
        };
        let mut request = client.post(self.endpoint()).json(body);
        if body.stream {
            request = request
                .header(ACCEPT, "text/event-stream")
                .header(ACCEPT_ENCODING, "identity");
        }
        for (name, value) in extra_headers {
            let header_name = match HeaderName::try_from(name.clone()) {
                Ok(h) => h,
                Err(_) => {
                    tracing::warn!("Skipping invalid extra header name: {:?}", name);
                    continue;
                }
            };
            let header_value = match HeaderValue::from_str(value) {
                Ok(v) => v,
                Err(_) => {
                    tracing::warn!(
                        "Skipping extra header with invalid value for {:?}",
                        header_name
                    );
                    continue;
                }
            };
            request = request.header(header_name, header_value);
        }

        request
    }
}

fn build_http_client(
    headers: &HeaderMap,
    timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
) -> Result<reqwest::Client, LlmError> {
    let mut builder = reqwest::Client::builder().default_headers(headers.clone());

    if let Some(timeout) = timeout {
        builder = builder.timeout(timeout);
    }
    if let Some(read_timeout) = read_timeout {
        builder = builder.read_timeout(read_timeout);
    }
    if let Some(connect_timeout) = connect_timeout {
        builder = builder.connect_timeout(connect_timeout);
    }

    builder.build().map_err(|e| LlmError::RequestFailed {
        provider: "openai-compatible".to_string(),
        reason: format!("failed to build HTTP client: {e}"),
    })
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

    fn context_window(&self) -> u32 {
        self.context_window
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let extra_headers = request.extra_headers.clone();
        let body = ChatCompletionsRequest::from_completion_request(&self.model, request, false);
        let response = self.send_chat_request(&body, &extra_headers).await?;
        let payload: ChatCompletionResponse =
            response
                .json()
                .await
                .map_err(|e| LlmError::InvalidResponse {
                    provider: "openai-compatible".to_string(),
                    reason: e.to_string(),
                })?;

        // Log the full payload for debugging provider test issues (before any moves)
        tracing::trace!(
            payload = ?payload,
            "full openai-compatible response payload"
        );

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

        tracing::info!(
            provider = "openai-compatible",
            input_tokens = usage.prompt_tokens,
            output_tokens = usage.completion_tokens,
            total_tokens = usage.prompt_tokens + usage.completion_tokens,
            "openai-compatible token usage"
        );

        // Log the raw response for debugging
        tracing::debug!(
            content = ?choice.message.content,
            reasoning_content = ?choice.message.reasoning_content,
            tool_calls = ?choice.message.tool_calls,
            finish_reason = ?choice.finish_reason,
            "openai-compatible complete response"
        );

        Ok(CompletionResponse {
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
        let extra_headers = request.extra_headers.clone();
        let body = ChatCompletionsRequest::from_completion_request(&self.model, request, true);
        let response = self.send_chat_request(&body, &extra_headers).await?;
        let stream = build_sse_event_stream("stream_complete", response.bytes_stream());

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
            tools: request
                .tools
                .map(|tools| tools.into_iter().map(OpenAiToolDefinition::from).collect()),
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
    reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

impl From<argus_protocol::llm::ChatMessage> for OpenAiMessage {
    fn from(value: argus_protocol::llm::ChatMessage) -> Self {
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
            reasoning_content: value.reasoning_content,
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
    reasoning_content: Option<String>,
    content: Option<String>,
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

fn parse_stream_frame_impl(
    data: &str,
) -> Result<Vec<Result<LlmStreamEvent, LlmError>>, serde_json::Error> {
    if data == "[DONE]" {
        return Ok(Vec::new());
    }

    let chunk: ChatCompletionChunk = serde_json::from_str(data)?;

    let mut events = Vec::new();

    if let Some(usage) = chunk.usage {
        tracing::info!(
            provider = "openai-compatible",
            input_tokens = usage.prompt_tokens,
            output_tokens = usage.completion_tokens,
            total_tokens = usage.prompt_tokens + usage.completion_tokens,
            "openai-compatible stream token usage"
        );
        events.push(Ok(LlmStreamEvent::Usage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
        }));
    }

    for choice in chunk.choices {
        if let Some(reasoning) = choice.delta.reasoning_content
            && !reasoning.is_empty()
        {
            events.push(Ok(LlmStreamEvent::ReasoningDelta { delta: reasoning }));
        }

        if let Some(content) = choice.delta.content
            && !content.is_empty()
        {
            events.push(Ok(LlmStreamEvent::ContentDelta { delta: content }));
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

    Ok(events)
}

fn parse_stream_frame(data: &str) -> Vec<Result<LlmStreamEvent, LlmError>> {
    match parse_stream_frame_impl(data) {
        Ok(events) => events,
        Err(e) => {
            tracing::error!(
                error = %e,
                raw_sse_frame = %data,
                "failed to parse SSE frame as JSON; skipping malformed frame"
            );
            Vec::new()
        }
    }
}

fn parse_final_stream_frame(data: &str) -> Vec<Result<LlmStreamEvent, LlmError>> {
    match parse_stream_frame_impl(data) {
        Ok(events) => events,
        Err(e) => {
            tracing::error!(
                error = %e,
                raw_sse_frame = %data,
                "failed to parse trailing SSE frame as JSON; treating stream as truncated"
            );
            vec![Err(LlmError::StreamInterrupted {
                provider: "openai-compatible".to_string(),
                reason: format!("truncated SSE frame at end of stream: {e}"),
            })]
        }
    }
}

fn parse_finish_reason(reason: Option<&str>) -> argus_protocol::llm::FinishReason {
    tracing::debug!(raw_finish_reason = ?reason, "parse_finish_reason");
    match reason {
        Some("stop") => argus_protocol::llm::FinishReason::Stop,
        Some("length") => argus_protocol::llm::FinishReason::Length,
        Some("tool_calls") => argus_protocol::llm::FinishReason::ToolUse,
        Some("content_filter") => argus_protocol::llm::FinishReason::ContentFilter,
        // BigModel uses "model_context_window_exceeded" for context length exceeded
        Some("model_context_window_exceeded") => argus_protocol::llm::FinishReason::Length,
        _ => argus_protocol::llm::FinishReason::Unknown,
    }
}

async fn map_http_error(response: reqwest::Response, model: &str) -> LlmError {
    let status = response.status();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs);
    let body = response.text().await.unwrap_or_default();

    classify_http_error(status, retry_after, &body, model)
}

fn classify_http_error(
    status: reqwest::StatusCode,
    retry_after: Option<Duration>,
    body: &str,
    model: &str,
) -> LlmError {
    let lower_body = body.to_ascii_lowercase();

    match status.as_u16() {
        401 | 403 => LlmError::AuthFailed {
            provider: "openai-compatible".to_string(),
            reason: format!("HTTP {}: {}", status, body),
        },
        404 if lower_body.contains("model") => LlmError::ModelNotAvailable {
            provider: "openai-compatible".to_string(),
            model: model.to_string(),
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
    use futures_util::stream;
    use reqwest::header::HeaderMap;
    use std::error::Error;
    use std::fmt;
    use tokio::net::TcpListener;
    use tokio::time::sleep;

    #[derive(Debug)]
    struct OuterError {
        source: InnerError,
    }

    #[derive(Debug)]
    struct InnerError;

    impl fmt::Display for OuterError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "outer")
        }
    }

    impl Error for OuterError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.source)
        }
    }

    impl fmt::Display for InnerError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "inner")
        }
    }

    impl Error for InnerError {}

    #[test]
    fn chat_completions_request_serializes_thinking() {
        let request = CompletionRequest::new(vec![argus_protocol::llm::ChatMessage::user("hi")])
            .with_thinking(ThinkingConfig::enabled().with_clear_thinking(false));

        let body = ChatCompletionsRequest::from_completion_request("glm-5", request, false);
        let json = serde_json::to_value(body).expect("request should serialize");

        assert_eq!(json["thinking"]["type"], "enabled");
        assert_eq!(json["thinking"]["clear_thinking"], false);
    }

    #[test]
    fn chat_completions_request_serializes_reasoning_content() {
        let request = CompletionRequest::new(vec![
            argus_protocol::llm::ChatMessage::assistant_with_reasoning(
                "final answer",
                Some("hidden reasoning".to_string()),
            ),
        ]);

        let body = ChatCompletionsRequest::from_completion_request("glm-5", request, false);
        let json = serde_json::to_value(body).expect("request should serialize");

        assert_eq!(json["messages"][0]["content"], "final answer");
        assert_eq!(json["messages"][0]["reasoning_content"], "hidden reasoning");
    }

    #[test]
    fn streaming_requests_set_sse_headers() {
        let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleConfig::new(
            "https://example.com/v1",
            "key",
            "glm-5",
        ))
        .expect("provider should build");
        let body = ChatCompletionsRequest::from_completion_request(
            "glm-5",
            CompletionRequest::new(vec![argus_protocol::llm::ChatMessage::user("hi")]),
            true,
        );

        let request = provider
            .build_chat_request(&body, &[])
            .build()
            .expect("request should build");

        assert_eq!(
            request
                .headers()
                .get(ACCEPT)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );
        assert_eq!(
            request
                .headers()
                .get(ACCEPT_ENCODING)
                .and_then(|value| value.to_str().ok()),
            Some("identity")
        );
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
                    finish_reason: argus_protocol::llm::FinishReason::Stop,
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
    fn config_defaults_to_120_second_timeout() {
        let config = OpenAiCompatibleConfig::new("https://example.com/v1", "key", "model");

        assert_eq!(config.timeout, Duration::from_secs(120));
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

    #[test]
    fn raw_stream_capture_keeps_only_tail_bytes() {
        let mut capture = RawStreamCapture::new(8);
        capture.push(b"hello");
        capture.push(b"-world");

        assert_eq!(capture.preview(), "lo-world");
    }

    #[test]
    fn error_source_chain_collects_nested_causes() {
        let error = OuterError { source: InnerError };

        assert_eq!(error_source_chain(&error), "outer <- inner");
    }

    #[tokio::test]
    async fn reqwest_error_diagnostics_marks_timeout_errors() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr");

        tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.expect("accept connection");
            sleep(Duration::from_millis(250)).await;
        });

        let client = build_http_client(
            &HeaderMap::new(),
            None,
            Some(Duration::from_millis(50)),
            Some(Duration::from_millis(50)),
        )
        .expect("build timeout client");

        let error = client
            .get(format!("http://{addr}"))
            .send()
            .await
            .expect_err("request should time out");

        let diagnostics = reqwest_error_diagnostics(&error);
        assert!(diagnostics.is_timeout);
        assert!(
            diagnostics.source_chain.contains("timed out")
                || diagnostics.source_chain.contains("timeout")
        );
    }

    #[test]
    fn sse_decoder_reassembles_split_frames() {
        let mut decoder = SseFrameDecoder::new();
        let first = decoder.push(&b"data: {\"choices\":"[..]);
        let second =
            decoder.push(&b"[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n"[..]);

        assert!(first.is_empty());
        let events = second
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("split SSE frame should parse");

        assert_eq!(
            events,
            vec![LlmStreamEvent::ContentDelta {
                delta: "hi".to_string(),
            }]
        );
    }

    #[test]
    fn sse_decoder_supports_carriage_return_frame_separator() {
        let mut decoder = SseFrameDecoder::new();
        let events =
            decoder.push(&b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\r\r"[..]);

        let events = events
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("CR-separated SSE frame should parse");

        assert_eq!(
            events,
            vec![LlmStreamEvent::ContentDelta {
                delta: "hi".to_string(),
            }]
        );
    }

    #[test]
    fn sse_decoder_skips_malformed_frame_and_continues() {
        let mut decoder = SseFrameDecoder::new();
        let events = decoder.push(
            &b"data: {\"choices\":[{\"delta\":{\"content\":\"ok-1\"},\"finish_reason\":null}]}\n\n\
data: {\"choices\":[{\"delta\":invalid-json}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"ok-2\"},\"finish_reason\":null}]}\n\n"[..],
        );

        let events = events
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("malformed frame should be skipped without failing the stream");

        assert_eq!(
            events,
            vec![
                LlmStreamEvent::ContentDelta {
                    delta: "ok-1".to_string(),
                },
                LlmStreamEvent::ContentDelta {
                    delta: "ok-2".to_string(),
                },
            ]
        );
    }
    #[tokio::test]
    async fn sse_event_stream_flushes_unterminated_final_frame() {
        let stream = stream::iter(vec![Ok::<Vec<u8>, reqwest::Error>(
            br#"data: {"choices":[{"delta":{"content":"tail"},"finish_reason":null}]}"#.to_vec(),
        )]);
        let events = SseEventStream::new("test", Box::pin(stream))
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("unterminated final SSE frame should still parse");

        assert_eq!(
            events,
            vec![LlmStreamEvent::ContentDelta {
                delta: "tail".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn sse_event_stream_reports_truncated_final_frame_as_stream_interrupted() {
        let stream = stream::iter(vec![Ok::<Vec<u8>, reqwest::Error>(
            br#"data: {"choices":[{"delta":{"content":"tail"#.to_vec(),
        )]);
        let events = SseEventStream::new("test", Box::pin(stream))
            .collect::<Vec<_>>()
            .await;

        assert_eq!(
            events.len(),
            1,
            "truncated frame should surface exactly one error"
        );
        assert!(matches!(
            &events[0],
            Err(LlmError::StreamInterrupted { provider, reason })
                if provider == "openai-compatible" && reason.contains("truncated SSE frame")
        ));
    }

    #[test]
    fn http_404_model_errors_map_to_model_not_available() {
        let error = classify_http_error(
            reqwest::StatusCode::NOT_FOUND,
            None,
            r#"{"error":"model not available"}"#,
            "gpt-4.1",
        );

        assert!(matches!(
            error,
            LlmError::ModelNotAvailable { provider, model }
            if provider == "openai-compatible" && model == "gpt-4.1"
        ));
    }
}
