#!/bin/bash
# Test script to demonstrate retry behavior in real streaming scenarios

set -e

# Get script directory and change to workspace root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$(git rev-parse --show-toplevel 2>/dev/null || echo "$SCRIPT_DIR/../..")"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║   Test Retry Behavior in Real Streaming                   ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

# Color codes
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}Test 1: Normal streaming (no test-retry)${NC}"
echo "This should work normally without any retries."
echo ""
cargo run --bin argus-llm -- complete --prompt "Count to 3" --stream 2>&1 | grep -E "(Test mode|Completing|Counting|📊|🔄)" || echo "Note: Running without test mode"
echo ""
echo "════════════════════════════════════════════════════════════"
echo ""

echo -e "${BLUE}Test 2: Streaming with --test-retry (first call succeeds)${NC}"
echo "The provider's pattern: Success → Fail → Fail → Fail → Success"
echo "First call should succeed, so no retries."
echo ""
cargo run --bin argus-llm -- complete --prompt "Say 'test 2'" --stream --test-retry --max-retries 3 2>&1 | grep -E "(🧪|📊|🔄|Completing|test 2)"
echo ""
echo "════════════════════════════════════════════════════════════"
echo ""

echo -e "${YELLOW}Test 3: Multiple calls to trigger retries${NC}"
echo "Making multiple calls to demonstrate the retry pattern."
echo "Call 1: Success (no retry)"
echo "Call 2: Should fail and retry"
echo "Call 3: Should fail and retry"
echo "Call 4: Should fail and retry"
echo "Call 5: Success"
echo ""

for i in 1 2 3 4 5; do
    echo -e "${BLUE}Call $i:${NC}"
    cargo run --bin argus-llm -- complete --prompt "echo $i" --test-retry --max-retries 3 2>&1 | grep -E "(🧪|🔄|📊|echo $i)" | head -3
    echo ""
    sleep 0.5
done

echo -e "${GREEN}✓ Tests completed!${NC}"
echo ""
echo "Key observations:"
echo "- Call 1 succeeded immediately (no retry events)"
echo "- Calls 2-4 should show retry attempts"
echo "- Call 5 should succeed again"
echo "- Each retry shows: 🔄 Retry attempt X/Y: error message"
echo "- Final summary shows: 📊 Total retries: N"
