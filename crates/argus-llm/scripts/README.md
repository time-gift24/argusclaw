# Argus-LLM Test Scripts

This directory contains test scripts for verifying retry behavior and LLM provider functionality.

## Scripts

### test-retry-behavior.sh
Automated test suite for retry behavior using mock providers.

**Tests:**
- IntermittentFailureProvider (success after 3 retries)
- AlwaysFailProvider (failure after retry exhaustion)
- Unit tests for both providers
- Integration tests with argus-llm and argus-turn CLIs

**Usage:**
```bash
./scripts/test-retry-behavior.sh
```

### demo-retry-events.sh
Demonstration script showing retry events in real streaming scenarios.

**Demonstrates:**
- Insufficient retries (will fail)
- Sufficient retries (will succeed)
- Non-stream mode comparison
- Retry event display with emoji indicators

**Usage:**
```bash
./scripts/demo-retry-events.sh
```

### test-real-retry.sh
Test script using real LLM providers with injected failures.

**Tests:**
- Normal streaming (no test-retry)
- Streaming with --test-retry flag
- Multiple calls to demonstrate retry pattern
- Real provider integration

**Usage:**
```bash
./scripts/test-real-retry.sh
```

## Quick Start

### Test Mock Providers
```bash
cd crates/argus-llm
./scripts/test-retry-behavior.sh
```

### See Retry Events in Action
```bash
cd crates/argus-llm
./scripts/demo-retry-events.sh
```

### Test with Real Provider
```bash
cd crates/argus-llm
./scripts/test-real-retry.sh
```

## Manual Testing

### Basic retry test with mock provider
```bash
cargo run --bin argus-llm -- mock-test --test-type always-fail --max-retries 2
```

### Test retry events with streaming
```bash
cargo run --bin argus-llm -- complete --prompt "Say hello" --stream --test-retry --max-retries 5
```

### Test with real provider
```bash
cargo run --example test_real_retry
```

## Expected Output

### Retry Events
When retries occur, you'll see:
```
🔄 Retry attempt 1/3: Provider rate limited, retry after Some(100ms)
🔄 Retry attempt 2/3: Provider rate limited, retry after Some(100ms)
🔄 Retry attempt 3/3: Provider rate limited, retry after Some(100ms)
📊 Total retries: 3
```

### Success
```
✓ Stream finished: Stop
📊 Total retries: 3
```

### Failure
```
✗ Stream error: Provider rate limited
Error: Mock test failed after 3 retries
```

## Test Patterns

### IntermittentFailureProvider
- Call 1: Success
- Calls 2-4: Fail (RateLimited)
- Call 5+: Success

### AlwaysFailProvider
- All calls: Fail (RateLimited)

### TestRetryProvider (with --test-retry)
- Call 1: Fail → triggers retries
- Calls 2-4: Fail → continues retries
- Call 5+: Success

## Requirements

- Rust toolchain
- Cargo
- Working ARGUS_LLM_API_KEY environment variable (for real provider tests)

## Notes

- Mock providers don't require API keys
- Real provider tests need valid credentials
- Each retry has a 100ms delay for testing
- All scripts are self-contained and executable
