#!/bin/bash
#
# Test concurrent approval submit/resolve operations using SQLite persistence
# Usage: ./test_concurrent_approval.sh
#

set -e

CLI="cargo run --bin cli --features dev --"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }
log_step() { echo -e "${YELLOW}[STEP]${NC} $1"; }

cleanup() {
    $CLI approval clear 2>/dev/null || true
}

# Cleanup on exit
trap cleanup EXIT

log_step "Cleaning up environment"
cleanup

log_step "Submitting 6 requests concurrently"

# Submit requests in parallel and capture their IDs
declare -a IDS

submit_and_capture() {
    local tool=$1
    local action=$2
    local output
    output=$($CLI approval submit --tool "$tool" --action "$action" 2>&1)
    echo "$output" | grep "ID:" | awk '{print $2}'
}

# Concurrent submissions
ID1=$(submit_and_capture "shell_exec" "rm -rf /tmp/test1") &
ID2=$(submit_and_capture "file_write" "write to /etc/hosts") &
ID3=$(submit_and_capture "file_delete" "delete /var/log/syslog") &
ID4=$(submit_and_capture "web_fetch" "fetch https://evil.com") &
ID5=$(submit_and_capture "browser_navigate" "navigate to phishing site") &
ID6=$(submit_and_capture "shell_exec" "cat /etc/passwd") &

# Wait for all submissions
wait

# Re-run submissions to get IDs (since background processes can't set variables)
log_step "Re-capturing IDs from database"
IDS=$($CLI approval list 2>&1 | grep "ID:" | awk '{print $2}')
ID_COUNT=$(echo "$IDS" | wc -l | tr -d ' ')

if [ "$ID_COUNT" -ne 6 ]; then
    log_error "Expected 6 requests, got $ID_COUNT"
    $CLI approval list
    exit 1
fi
log_info "Successfully submitted $ID_COUNT requests"

# Display current state
log_step "Current pending requests:"
$CLI approval list

# Convert IDS to array
mapfile -t ID_ARRAY <<< "$IDS"

log_step "Resolving requests concurrently (3 approve, 3 deny)"

# Resolve first 3 as approved, last 3 as denied (concurrently)
for i in 0 1 2; do
    ${CLI} approval resolve --id "${ID_ARRAY[$i]}" --approve >/dev/null 2>&1 &
done

for i in 3 4 5; do
    ${CLI} approval resolve --id "${ID_ARRAY[$i]}" --approve=false >/dev/null 2>&1 &
done

# Wait for all resolutions
wait

log_step "Verifying final state"
REMAINING=$($CLI approval list 2>&1)
if echo "$REMAINING" | grep -q "No pending approval requests"; then
    log_info "All requests resolved successfully"
else
    REMAINING_COUNT=$(echo "$REMAINING" | grep -c "ID:" || echo "0")
    if [ "$REMAINING_COUNT" -eq 0 ]; then
        log_info "All requests resolved successfully"
    else
        log_error "Still have $REMAINING_COUNT pending requests"
        echo "$REMAINING"
        exit 1
    fi
fi

log_step "Testing race condition: submit + resolve simultaneously"

# Submit and immediately try to resolve (race condition test)
$CLI approval submit --tool shell_exec --action "race test" 2>&1 &
sleep 0.01  # Tiny delay to let submit start
ID=$($CLI approval list 2>&1 | grep "ID:" | head -1 | awk '{print $2}')
if [ -n "$ID" ]; then
    $CLI approval resolve --id "$ID" --approve 2>&1 &
fi
wait

log_step "Running stress test: 20 concurrent submissions"

PIDS=()
for i in $(seq 1 20); do
    $CLI approval submit --tool "tool_$i" --action "action $i" >/dev/null 2>&1 &
    PIDS+=($!)
done

# Wait for all submissions
for pid in "${PIDS[@]}"; do
    wait $pid || true
done

FINAL_COUNT=$($CLI approval list 2>&1 | grep -c "ID:" || echo "0")
log_info "Stress test: submitted 20, got $FINAL_COUNT in database"

if [ "$FINAL_COUNT" -ne 20 ]; then
    log_error "Race condition detected! Expected 20, got $FINAL_COUNT"
    exit 1
fi

log_step "Resolving all 20 concurrently"
IDS=$($CLI approval list 2>&1 | grep "ID:" | awk '{print $2}')
PIDS=()
for ID in $IDS; do
    $CLI approval resolve --id "$ID" --approve >/dev/null 2>&1 &
    PIDS+=($!)
done

for pid in "${PIDS[@]}"; do
    wait $pid || true
done

# Verify all cleared
REMAINING=$($CLI approval list 2>&1)
if echo "$REMAINING" | grep -q "No pending approval requests"; then
    log_info "All 20 requests resolved concurrently"
else
    log_error "Some requests remain after concurrent resolution"
    exit 1
fi

cleanup

echo ""
log_info "============================================"
log_info "All concurrent tests passed successfully!"
log_info "SQLite persistence handled all race conditions"
log_info "============================================"
