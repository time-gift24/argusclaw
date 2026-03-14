#!/usr/bin/env bash
#
# Repeatable approval flow test via dev CLI.
# Verifies:
# 1) approval decisions are controllable (approve/deny/timeout)
# 2) concurrent multi-agent submit/resolve works with SQLite persistence
# 3) resolve decisions are surfaced in top-level CLI output
#

set -euo pipefail
shopt -s nullglob

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_ROOT="${ROOT_DIR}/tmp"
RUN_ID="$(date +%Y%m%d-%H%M%S)-$$"
TMP_DIR="${TMP_ROOT}/approval-test-${RUN_ID}"
mkdir -p "${TMP_DIR}"
DB_FILE="${TMP_DIR}/approval.sqlite"
export APPROVAL_DATABASE_URL="sqlite:${DB_FILE}"

CLI_BIN="${ROOT_DIR}/target/debug/argusclaw-dev"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_step() { echo -e "${YELLOW}[STEP]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1" >&2; }

cleanup() {
    if [[ -x "${CLI_BIN}" ]]; then
        "${CLI_BIN}" approval clear >/dev/null 2>&1 || true
    fi
    rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

build_cli() {
    log_step "Building dev CLI binary"
    (cd "${ROOT_DIR}" && cargo build -p cli --features dev >/dev/null)
}

cli() {
    "${CLI_BIN}" "$@"
}

extract_id() {
    awk '/^[[:space:]]*ID:[[:space:]]/ {print $2; exit}'
}

count_pending_ids() {
    awk '/^[[:space:]]*ID:[[:space:]]/ {count++} END {print count + 0}'
}

count_agent_entries() {
    local agent="$1"
    awk -v target="${agent}" '
        /^[[:space:]]*Agent:[[:space:]]/ {
            if ($2 == target) count++
        }
        END { print count + 0 }
    '
}

build_cli
log_info "Workspace: ${ROOT_DIR}"
log_info "Run ID: ${RUN_ID}"
log_info "Approval test database: ${DB_FILE}"

log_step "Resetting pending approval requests in isolated test DB"
cli approval clear >/dev/null || true

log_step "Validating controllable outcomes via approval test"
approve_out="$(cli approval test --agent control-agent --tool shell_exec --timeout 2 --approve)"
deny_out="$(cli approval test --agent control-agent --tool shell_exec --timeout 2 --deny)"
timeout_out="$(cli approval test --agent control-agent --tool shell_exec --timeout 1)"

if ! printf '%s\n' "${approve_out}" | grep -q "Result: APPROVED"; then
    log_error "Expected APPROVED result in control test"
    printf '%s\n' "${approve_out}" >&2
    exit 1
fi
if ! printf '%s\n' "${deny_out}" | grep -q "Result: DENIED"; then
    log_error "Expected DENIED result in control test"
    printf '%s\n' "${deny_out}" >&2
    exit 1
fi
if ! printf '%s\n' "${timeout_out}" | grep -q "Result: TIMED OUT"; then
    log_error "Expected TIMED OUT result in control test"
    printf '%s\n' "${timeout_out}" >&2
    exit 1
fi

log_info "Control checks passed: approve/deny/timeout are all reachable"

REQUESTS=(
    "agent-a|shell_exec|dangerous shell command A|approve"
    "agent-a|file_write|write sensitive file A|deny"
    "agent-b|file_delete|delete important file B|approve"
    "agent-b|web_fetch|fetch remote URL B|deny"
    "agent-c|browser_navigate|navigate suspicious URL C|approve"
    "agent-c|shell_exec|dangerous shell command C|deny"
)

expected_total="${#REQUESTS[@]}"

log_step "Submitting ${expected_total} requests concurrently across 3 agents"
for idx in "${!REQUESTS[@]}"; do
    spec="${REQUESTS[$idx]}"
    IFS='|' read -r agent tool action decision <<< "${spec}"

    (
        submit_out="$(cli approval submit --agent "${agent}" --tool "${tool}" --action "${action}" --timeout 120)"
        request_id="$(printf '%s\n' "${submit_out}" | extract_id)"
        if [[ -z "${request_id}" ]]; then
            log_error "Failed to extract request id for ${agent}/${tool}"
            printf '%s\n' "${submit_out}" >&2
            exit 1
        fi
        printf '%s|%s|%s|%s|%s\n' "${idx}" "${request_id}" "${agent}" "${tool}" "${decision}" > "${TMP_DIR}/request-${idx}.meta"
    ) &
done
wait

meta_files=( "${TMP_DIR}"/request-*.meta )
if [[ "${#meta_files[@]}" -ne "${expected_total}" ]]; then
    log_error "Expected ${expected_total} request metadata files, got ${#meta_files[@]}"
    exit 1
fi
log_info "Captured request metadata:"
for meta_file in "${meta_files[@]}"; do
    IFS='|' read -r idx request_id agent tool decision < "${meta_file}"
    log_info "  [${idx}] id=${request_id} agent=${agent} tool=${tool} decision=${decision}"
done

list_out="$(cli approval list)"
pending_count="$(printf '%s\n' "${list_out}" | count_pending_ids)"
if [[ "${pending_count}" -ne "${expected_total}" ]]; then
    log_error "Expected ${expected_total} pending requests, got ${pending_count}"
    printf '%s\n' "${list_out}" >&2
    exit 1
fi

for agent in agent-a agent-b agent-c; do
    expected_agent_count="$(printf '%s\n' "${REQUESTS[@]}" | awk -F'|' -v target="${agent}" '$1 == target {count++} END {print count + 0}')"
    actual_agent_count="$(printf '%s\n' "${list_out}" | count_agent_entries "${agent}")"
    if [[ "${actual_agent_count}" -ne "${expected_agent_count}" ]]; then
        log_error "Agent ${agent}: expected ${expected_agent_count} pending, got ${actual_agent_count}"
        printf '%s\n' "${list_out}" >&2
        exit 1
    fi
done

log_info "Multi-agent pending queue verified"
log_info "Pending entries by agent: agent-a=2, agent-b=2, agent-c=2"

log_step "Resolving all requests and validating top-level passthrough output"
for meta_file in "${meta_files[@]}"; do
    IFS='|' read -r idx request_id agent tool decision < "${meta_file}"
    if [[ "${decision}" == "approve" ]]; then
        resolve_out="$(cli approval resolve --id "${request_id}" --approve)"
        expected_word="Approved"
    else
        resolve_out="$(cli approval resolve --id "${request_id}")"
        expected_word="Denied"
    fi

    printf '%s\n' "${resolve_out}" > "${TMP_DIR}/resolve-${idx}.out"

    if ! printf '%s\n' "${resolve_out}" | grep -q "${request_id}"; then
        log_error "Resolve output does not include request id ${request_id}"
        printf '%s\n' "${resolve_out}" >&2
        exit 1
    fi
    if ! printf '%s\n' "${resolve_out}" | grep -q "${expected_word}"; then
        log_error "Resolve output does not include decision ${expected_word} for ${request_id}"
        printf '%s\n' "${resolve_out}" >&2
        exit 1
    fi
    log_info "  resolved id=${request_id} expected=${expected_word}"
done

approved_expected="$(printf '%s\n' "${REQUESTS[@]}" | awk -F'|' '$4 == "approve" {count++} END {print count + 0}')"
denied_expected="$(printf '%s\n' "${REQUESTS[@]}" | awk -F'|' '$4 == "deny" {count++} END {print count + 0}')"
approved_actual="$(grep -h -c "Approved" "${TMP_DIR}"/resolve-*.out | awk '{sum += $1} END {print sum + 0}')"
denied_actual="$(grep -h -c "Denied" "${TMP_DIR}"/resolve-*.out | awk '{sum += $1} END {print sum + 0}')"

if [[ "${approved_actual}" -ne "${approved_expected}" ]]; then
    log_error "Expected ${approved_expected} approved outputs, got ${approved_actual}"
    exit 1
fi
if [[ "${denied_actual}" -ne "${denied_expected}" ]]; then
    log_error "Expected ${denied_expected} denied outputs, got ${denied_actual}"
    exit 1
fi

final_list="$(cli approval list)"
final_pending="$(printf '%s\n' "${final_list}" | count_pending_ids)"
if [[ "${final_pending}" -ne 0 ]]; then
    log_error "Expected 0 pending requests after concurrent resolution, got ${final_pending}"
    printf '%s\n' "${final_list}" >&2
    exit 1
fi

log_info "All approval requests resolved successfully"
log_info "Summary: approved=${approved_actual}, denied=${denied_actual}, final_pending=${final_pending}"
log_info "Repeatable multi-agent approval test passed"
