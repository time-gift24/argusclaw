#!/bin/bash
# Test script for argus-turn CLI tool
# This script tests the tool calling functionality

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}=== argus-turn CLI Test Script ===${NC}"
echo ""

# Check if environment variables are set
if [ -z "$ARGUS_LLM_API_KEY" ]; then
    echo -e "${RED}Error: ARGUS_LLM_API_KEY is not set${NC}"
    echo "Please set: export ARGUS_LLM_API_KEY=your_api_key"
    exit 1
fi

# Set defaults if not provided
: ${ARGUS_LLM_BASE_URL:="https://api.openai.com/v1"}
: ${ARGUS_LLM_MODEL:="gpt-4o-mini"}

echo -e "${GREEN}✓ ARGUS_LLM_API_KEY is set${NC}"
echo -e "${GREEN}✓ ARGUS_LLM_BASE_URL=${ARGUS_LLM_BASE_URL}${NC}"
echo -e "${GREEN}✓ ARGUS_LLM_MODEL=${ARGUS_LLM_MODEL}${NC}"
echo ""

# Build the CLI
echo -e "${YELLOW}Building argus-turn CLI...${NC}"
cargo build --bin argus-turn --quiet
echo -e "${GREEN}✓ Build complete${NC}"
echo ""

# Test 1: Simple execution without tools
echo -e "${YELLOW}=== Test 1: Simple Turn Execution ===${NC}"
echo "Prompt: What is 2+2?"
cargo run --bin argus-turn -- execute \
    --base-url "$ARGUS_LLM_BASE_URL" \
    --api-key "$ARGUS_LLM_API_KEY" \
    --model "$ARGUS_LLM_MODEL" \
    --prompt "What is 2+2? Reply with just the number."
echo ""

# Test 2: Tool execution test
echo -e "${YELLOW}=== Test 2: Tool Execution Test ===${NC}"
echo "Prompt: Use the echo tool to echo 'Hello from argus-turn!'"
cargo run --bin argus-turn -- tool-test \
    --base-url "$ARGUS_LLM_BASE_URL" \
    --api-key "$ARGUS_LLM_API_KEY" \
    --model "$ARGUS_LLM_MODEL" \
    --prompt "Please use the echo tool to echo the message 'Hello from argus-turn!'. After receiving the result, tell me what was echoed."
echo ""

# Test 3: Multiple tool calls
echo -e "${YELLOW}=== Test 3: Multiple Tool Calls ===${NC}"
echo "Prompt: Echo two different messages"
cargo run --bin argus-turn -- tool-test \
    --base-url "$ARGUS_LLM_BASE_URL" \
    --api-key "$ARGUS_LLM_API_KEY" \
    --model "$ARGUS_LLM_MODEL" \
    --prompt "Use the echo tool twice: first echo 'Message 1', then echo 'Message 2'. Tell me both results."
echo ""

echo -e "${GREEN}=== All tests completed ===${NC}"
