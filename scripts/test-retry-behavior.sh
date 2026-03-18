#!/bin/bash
# Test script for retry behavior using mock providers
# Tests argus-llm and argus-turn CLIs with IntermittentFailureProvider and AlwaysFailProvider

set -e

echo "╔════════════════════════════════════════════════════════════╗"
echo "║   Retry Behavior Test Suite                                ║"
echo "║   Testing argus-test-support mock providers                ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

test_count=0
pass_count=0
fail_count=0

run_test() {
    local test_name="$1"
    local command="$2"
    local should_fail="$3"
    local expected_output="$4"

    test_count=$((test_count + 1))
    echo -e "${BLUE}[$test_count] Testing: $test_name${NC}"

    local output
    output=$(eval "$command" 2>&1)
    local exit_code=$?

    # Check for expected output if provided
    if [ -n "$expected_output" ]; then
        if echo "$output" | grep -q "$expected_output"; then
            echo -e "  ${GREEN}✓ Found expected output: $expected_output${NC}"
        else
            echo -e "  ${RED}✗ Missing expected output: $expected_output${NC}"
            echo -e "  ${YELLOW}Output:${NC}\n$output"
            fail_count=$((fail_count + 1))
            return
        fi
    fi

    # Check exit code
    if [ $exit_code -eq 0 ]; then
        if [ "$should_fail" = "true" ]; then
            echo -e "  ${RED}✗ FAIL: Expected failure but succeeded${NC}"
            fail_count=$((fail_count + 1))
        else
            echo -e "  ${GREEN}✓ PASS: Succeeded as expected${NC}"
            pass_count=$((pass_count + 1))
        fi
    else
        if [ "$should_fail" = "true" ]; then
            echo -e "  ${GREEN}✓ PASS: Failed as expected${NC}"
            pass_count=$((pass_count + 1))
        else
            echo -e "  ${RED}✗ FAIL: Expected success but failed${NC}"
            fail_count=$((fail_count + 1))
        fi
    fi
    echo ""
}

# Build binaries first
echo -e "${YELLOW}Building binaries...${NC}"
cargo build --bin argus-llm --bin argus-turn -q
echo -e "${GREEN}✓ Build complete${NC}"
echo ""

# ═════════════════════════════════════════════════════════════
# argus-llm CLI Tests
# ═════════════════════════════════════════════════════════════
echo -e "${BLUE}═══ argus-llm CLI Tests ═══${NC}"
echo ""

run_test \
    "argus-llm: Intermittent failure with max_retries=3" \
    "cargo run --bin argus-llm -- mock-test --test-type intermittent --max-retries 3" \
    "false" \
    "Stream finished"

run_test \
    "argus-llm: Intermittent failure with max_retries=5" \
    "cargo run --bin argus-llm -- mock-test --test-type intermittent --max-retries 5" \
    "false" \
    "Stream finished"

run_test \
    "argus-llm: Always fail with max_retries=1 (check retry events)" \
    "cargo run --bin argus-llm -- mock-test --test-type always-fail --max-retries 1" \
    "true" \
    "🔄 Retry attempt 1/1"

run_test \
    "argus-llm: Always fail with max_retries=3 (check retry events)" \
    "cargo run --bin argus-llm -- mock-test --test-type always-fail --max-retries 3" \
    "true" \
    "📊 Total retries: 3"

# ═════════════════════════════════════════════════════════════
# argus-turn CLI Tests
# ═════════════════════════════════════════════════════════════
echo -e "${BLUE}═══ argus-turn CLI Tests ═══${NC}"
echo ""

run_test \
    "argus-turn: Intermittent failure with max_retries=3" \
    "cargo run --bin argus-turn -- mock-test --test-type intermittent --max-retries 3" \
    "false" \
    "Turn completed successfully"

run_test \
    "argus-turn: Intermittent failure with max_retries=5" \
    "cargo run --bin argus-turn -- mock-test --test-type intermittent --max-retries 5" \
    "false" \
    "Turn completed successfully"

run_test \
    "argus-turn: Always fail with max_retries=1" \
    "cargo run --bin argus-turn -- mock-test --test-type always-fail --max-retries 1" \
    "true" \
    "Turn failed"

run_test \
    "argus-turn: Always fail with max_retries=3" \
    "cargo run --bin argus-turn -- mock-test --test-type always-fail --max-retries 3" \
    "true" \
    "Turn failed"

# ═════════════════════════════════════════════════════════════
# Unit Tests
# ═════════════════════════════════════════════════════════════
echo -e "${BLUE}═══ Unit Tests ═══${NC}"
echo ""

run_test \
    "argus-test-support: Unit tests" \
    "cargo test -p argus-test-support -q" \
    "false" \
    ""

# ═════════════════════════════════════════════════════════════
# Summary
# ═════════════════════════════════════════════════════════════
echo "╔════════════════════════════════════════════════════════════╗"
echo "║   Test Summary                                              ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""
echo "Total tests:  $test_count"
echo -e "${GREEN}Passed:       $pass_count${NC}"
echo -e "${RED}Failed:       $fail_count${NC}"
echo ""

if [ $fail_count -eq 0 ]; then
    echo -e "${GREEN}✓ All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}✗ Some tests failed${NC}"
    exit 1
fi
