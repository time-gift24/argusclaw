#!/bin/bash
# Workflow CLI 测试脚本
# 用法: ./scripts/test-workflow.sh

set -e

cd "$(dirname "$0")/.."

# 构建带 dev feature 的 CLI
echo "Building CLI with dev feature..."
cargo build --features dev -p cli --quiet

CLI="./target/debug/cli"
DB_PATH="./tmp/workflow-dev.sqlite"

# 清理之前的测试数据
rm -f "$DB_PATH"

echo ""
echo "=== Workflow CLI Test ==="
echo ""

# 1. 创建 workflow
echo "1. Creating workflow..."
OUTPUT=$($CLI workflow create "test-pipeline")
WORKFLOW_ID=$(echo "$OUTPUT" | grep "ID:" | awk '{print $2}')
echo "$OUTPUT"
echo ""

# 2. 添加 stages
echo "2. Adding stages..."
STAGE1_OUTPUT=$($CLI workflow add-stage --workflow "$WORKFLOW_ID" "prepare" 1)
STAGE1_ID=$(echo "$STAGE1_OUTPUT" | grep "ID:" | awk '{print $2}')
echo "$STAGE1_OUTPUT"

STAGE2_OUTPUT=$($CLI workflow add-stage --workflow "$WORKFLOW_ID" "build" 2)
STAGE2_ID=$(echo "$STAGE2_OUTPUT" | grep "ID:" | awk '{print $2}')
echo "$STAGE2_OUTPUT"

STAGE3_OUTPUT=$($CLI workflow add-stage --workflow "$WORKFLOW_ID" "deploy" 3)
STAGE3_ID=$(echo "$STAGE3_OUTPUT" | grep "ID:" | awk '{print $2}')
echo "$STAGE3_OUTPUT"
echo ""

# 3. 插入测试 agent 和 provider (绕过外键约束)
echo "3. Setting up test agents..."
# 先插入 llm_providers (使用正确的字段)
sqlite3 "$DB_PATH" "INSERT INTO llm_providers (id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce) VALUES ('test-provider', 'openai_compatible', 'Test Provider', 'https://api.test.com', 'test-model', X'00', X'00');"

# 再插入 agents
sqlite3 "$DB_PATH" "INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('fetcher', 'Fetcher Agent', 'test-provider', 'You are a fetcher.');"
sqlite3 "$DB_PATH" "INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('validator', 'Validator Agent', 'test-provider', 'You are a validator.');"
sqlite3 "$DB_PATH" "INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('builder', 'Builder Agent', 'test-provider', 'You are a builder.');"
sqlite3 "$DB_PATH" "INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('tester', 'Tester Agent', 'test-provider', 'You are a tester.');"
sqlite3 "$DB_PATH" "INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('deployer', 'Deployer Agent', 'test-provider', 'You are a deployer.');"
echo "Done."
echo ""

# 4. 添加 jobs
echo "4. Adding jobs..."
echo "$($CLI workflow add-job --stage "$STAGE1_ID" --agent "fetcher" "fetch-data")"
echo "$($CLI workflow add-job --stage "$STAGE1_ID" --agent "validator" "validate-input")"
echo "$($CLI workflow add-job --stage "$STAGE2_ID" --agent "builder" "compile")"
echo "$($CLI workflow add-job --stage "$STAGE2_ID" --agent "tester" "run-tests")"
echo "$($CLI workflow add-job --stage "$STAGE3_ID" --agent "deployer" "push-artifacts")"
echo ""

# 5. 列出所有 workflows
echo "5. Listing all workflows..."
$CLI workflow list | grep -A2 "test-pipeline" || true
echo ""

# 6. 查看状态 (树形输出)
echo "6. Workflow status (tree view):"
echo "────────────────────────────────────────"
$CLI workflow status "$WORKFLOW_ID"
echo "────────────────────────────────────────"
echo ""

# 7. 更新部分 job 状态
echo "7. Updating job statuses (simulating execution)..."
# 获取 job IDs
JOB1_ID=$(sqlite3 "$DB_PATH" "SELECT id FROM jobs WHERE name='fetch-data' LIMIT 1;")
JOB2_ID=$(sqlite3 "$DB_PATH" "SELECT id FROM jobs WHERE name='validate-input' LIMIT 1;")
JOB3_ID=$(sqlite3 "$DB_PATH" "SELECT id FROM jobs WHERE name='compile' LIMIT 1;")
JOB4_ID=$(sqlite3 "$DB_PATH" "SELECT id FROM jobs WHERE name='run-tests' LIMIT 1;")

$CLI workflow job-status --id "$JOB1_ID" succeeded
$CLI workflow job-status --id "$JOB2_ID" running
echo ""

# 8. 测试状态回退保护 (succeeded -> running 应该失败)
echo "8. Testing status rollback protection..."
echo "Attempt: succeeded -> running (should fail)..."
if $CLI workflow job-status --id "$JOB1_ID" running 2>&1; then
    echo "ERROR: Should have failed!"
else
    echo "✓ Correctly rejected invalid transition"
fi
echo ""

# 9. 测试 failed 状态
echo "9. Testing failed status..."
$CLI workflow job-status --id "$JOB3_ID" running
$CLI workflow job-status --id "$JOB3_ID" failed
$CLI workflow job-status --id "$JOB4_ID" cancelled
echo ""

# 10. 最终状态
echo "10. Final workflow status:"
echo "────────────────────────────────────────"
$CLI workflow status "$WORKFLOW_ID"
echo "────────────────────────────────────────"

echo ""
echo "=== Status Color Legend ==="
echo "  ○ pending    (yellow)"
echo "  ⟳ running    (cyan)"
echo "  ✓ succeeded  (green)"
echo "  ✗ failed     (red)"
echo "  ⊘ cancelled  (dimmed)"
echo ""
echo "=== Test Complete ==="
echo ""
echo "Workflow ID: $WORKFLOW_ID"
echo "Database: $DB_PATH"
echo ""
echo "To inspect manually:"
echo "  $CLI workflow status $WORKFLOW_ID"
echo "  sqlite3 $DB_PATH '.tables'"
