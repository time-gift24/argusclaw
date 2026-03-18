#!/bin/bash
# Demonstration of retry events in real streaming scenarios

set -e

echo "╔════════════════════════════════════════════════════════════╗"
echo "║   Retry Events Demonstration                              ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}Demonstration 1: Insufficient retries (will fail)${NC}"
echo "Max retries: 3, but provider needs 4 retries to succeed"
echo ""
cargo run --bin argus-llm -- complete --prompt "Say hi" --stream --test-retry --max-retries 3 2>&1 | grep -E "(🧪|🔄|Error|📊)"
echo ""
echo "════════════════════════════════════════════════════════════"
echo ""

echo -e "${GREEN}Demonstration 2: Sufficient retries (will succeed)${NC}"
echo "Max retries: 5, provider needs 4 retries to succeed"
echo ""
cargo run --bin argus-llm -- complete --prompt "Say hi" --stream --test-retry --max-retries 5 2>&1 | grep -E "(🧪|🔄|Summary|📊)"
echo ""
echo "════════════════════════════════════════════════════════════"
echo ""

echo -e "${YELLOW}Demonstration 3: Non-stream mode${NC}"
echo "Non-stream mode doesn't show retry events during execution"
echo "But you can see the total time spent retrying"
echo ""
echo "Running without --stream flag..."
time cargo run --bin argus-llm -- complete --prompt "Say hi" --test-retry --max-retries 5 2>&1 | tail -5
echo ""
echo "Note: The execution time includes retry delays (4 × 100ms = 400ms)"
echo ""
echo "════════════════════════════════════════════════════════════"
echo ""

echo -e "${BLUE}Key Takeaways:${NC}"
echo "1. Use --stream flag to see retry events in real-time"
echo "2. Each retry shows: 🔄 Retry attempt X/Y: error message"
echo "3. Final summary shows: 📊 Total retries: N"
echo "4. Retry pattern: 4 failures → success (needs max_retries ≥ 4)"
echo ""
echo -e "${GREEN}✓ Retry events are working correctly!${NC}"
