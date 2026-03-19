#!/bin/bash

# 测试 LLM Provider 连接并显示详细日志
# 用法: ./test-provider-debug.sh <base_url> <api_key> <model>

set -e

BASE_URL=${1:-"https://api.openai.com/v1"}
API_KEY=${2:-"your-api-key-here"}
MODEL=${3:-"gpt-4o-mini"}

echo "🔍 Testing Provider Connection"
echo "Base URL: $BASE_URL"
echo "Model: $MODEL"
echo "---"

# 设置日志级别为 trace 以看到所有调试信息
RUST_LOG=arguswing=trace,argus=trace,argus_llm=trace \
  cargo run --package argus-llm --bin cli -- test \
    --base-url "$BASE_URL" \
    --api-key "$API_KEY" \
    --model "$MODEL" \
    2>&1 | tee /tmp/provider-test-debug.log

echo "---"
echo "📋 Full log saved to: /tmp/provider-test-debug.log"

# 提取关键信息
echo "🔑 Key Information:"
grep -E "(provider test received response|content_preview|openai-compatible complete response|full openai-compatible response payload)" /tmp/provider-test-debug.log || echo "No key information found in logs"
