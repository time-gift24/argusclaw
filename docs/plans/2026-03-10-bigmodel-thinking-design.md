# BigModel Thinking Support Design

## Summary

Add first-class support for BigModel/Z.AI thinking mode in the shared LLM abstraction without changing the current agent loop behavior.

This design is intentionally limited to upward passthrough:
- requests may include a `thinking` config
- responses may expose `reasoning_content`
- streaming may expose `reasoning_content` deltas
- providers explicitly report whether thinking is supported

This does not yet implement preserved thinking, history replay of reasoning blocks, or any UI/CLI presentation changes.

## Goals

- Support BigModel `thinking` request parameters on chat completion requests.
- Expose `reasoning_content` from non-streaming responses.
- Expose `reasoning_content` from streaming responses.
- Make provider thinking support explicit instead of implicit.
- Preserve backwards compatibility for existing callers that do not use thinking.

## Non-Goals

- No preserved thinking support (`clear_thinking = false` semantics stay provider-pass-through only).
- No storing `reasoning_content` in conversation history.
- No integration with the existing agent reasoning loop.
- No new provider kind for BigModel at this stage.
- No CLI or UI rendering changes unless already required by compile-time consumers.

## Constraints

- The codebase currently has a single provider kind, `openai-compatible`.
- BigModel is accessed through an OpenAI-compatible endpoint shape, so a separate provider kind would add complexity without immediate benefit.
- The existing public LLM types do not have a place for thinking config or reasoning output.

## Options Considered

### Option A: First-class provider-agnostic thinking support

Add `thinking` to shared request types, `reasoning_content` to shared response types, a `ReasoningDelta` stream event, and explicit provider capabilities.

Pros:
- Clear API surface
- Minimal ambiguity for callers
- Extensible for future preserved thinking support

Cons:
- Requires touching shared LLM types and tests

### Option B: Provider-specific extensions hidden behind generic metadata

Keep shared types mostly unchanged and pass thinking/reasoning through provider-specific extensions or metadata.

Pros:
- Smaller immediate diff

Cons:
- Weak typing
- Harder to evolve
- Consumers cannot rely on stable fields

### Option C: Separate BigModel provider kind

Introduce a new provider kind with its own request and response handling.

Pros:
- Capability boundary is explicit

Cons:
- More manager and storage complexity
- Not justified while BigModel still fits the existing OpenAI-compatible transport

## Decision

Use Option A.

The shared LLM abstraction will gain first-class types for thinking requests and reasoning passthrough, while provider support remains opt-in through explicit capabilities.

## Proposed API Changes

### Shared request types

Add `thinking: Option<ThinkingConfig>` to:
- `CompletionRequest`
- `ToolCompletionRequest`

Introduce:
- `ThinkingConfig`
- `ThinkingMode`

`ThinkingConfig` will model the BigModel request shape:
- `type`: enabled or disabled
- `clear_thinking`: boolean

The field remains optional so existing callers are unaffected.

### Shared response types

Add `reasoning_content: Option<String>` to:
- `CompletionResponse`
- `ToolCompletionResponse`

This field is passthrough-only in this phase. The agent layer may choose to ignore it for now.

### Streaming API

Add a new `LlmStreamEvent` variant:
- `ReasoningDelta { delta: String }`

This maps directly to BigModel streaming payloads that emit `delta.reasoning_content`.

### Provider capabilities

Add:
- `ProviderCapabilities { thinking: bool }`
- `LlmProvider::capabilities() -> ProviderCapabilities`

Default behavior for providers that do not override this method:
- `thinking = false`

This allows upper layers to make explicit decisions instead of inferring support from model names or ad hoc behavior.

## Provider Behavior

### OpenAI-compatible provider

The existing OpenAI-compatible provider will be extended to:

- serialize `thinking` when present
- parse `message.reasoning_content` on non-streaming responses
- parse `delta.reasoning_content` on streaming responses
- report whether the current model supports thinking

Capability reporting will be model-aware, based on the configured or active model name.

Even when capability reporting says thinking is unsupported, the provider should still tolerate and passthrough `reasoning_content` if the remote service returns it.

## Compatibility

- Existing callers that never set `thinking` keep current behavior.
- Existing providers continue to compile through default capability behavior.
- Existing content and tool-call parsing remains unchanged.
- New fields are additive and optional.

## Testing Strategy

### Request serialization

Verify that requests with `thinking` produce the expected JSON body for the OpenAI-compatible provider.

### Non-streaming response parsing

Verify that `message.reasoning_content` is captured on:
- plain chat completions
- tool-capable chat completions

### Streaming parsing

Verify that SSE frames with `delta.reasoning_content` emit `LlmStreamEvent::ReasoningDelta`.

### Capability reporting

Verify that supported models report `thinking = true` and unsupported ones report `false`.

### Regression coverage

Keep existing parsing tests for:
- `content`
- `tool_calls`
- `finish_reason`
- usage accounting

## Open Questions Deferred

- How `reasoning_content` should be displayed to users
- Whether preserved thinking should be enabled per provider, per model, or per request policy
- Whether historical `reasoning_content` belongs in `ChatMessage` or in a separate conversation artifact

## Implementation Boundary

This design intentionally stops at typed passthrough. It creates the right abstraction boundary now so preserved thinking and higher-level agent integration can be added later without redesigning the provider interface.
