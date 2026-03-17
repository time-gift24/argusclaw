#!/bin/bash
# Workflow CLI 测试脚本 (新版 - 使用 Job 模型)
# 用法: ./scripts/test-workflow.sh

set -e

cd "$(dirname "$0")/.."

# 构建带 dev feature 的 CLI
echo "Building CLI with dev feature..."
cargo build --features dev -p cli --bin arguswing-dev --quiet

CLI="./target/debug/arguswing-dev"
DB_PATH="$(pwd)/tmp/workflow-dev.sqlite"

# 清理之前的测试数据并创建 tmp 目录
rm -f "$DB_PATH"
mkdir -p "$(dirname "$DB_PATH")"

# 使用环境变量指定数据库路径
export WORKFLOW_DATABASE_URL="sqlite:$DB_PATH"

echo ""
echo "=== Workflow CLI Test (Job Model) ==="
echo ""

# 1. 创建 workflow (作为 group)
echo "1. Creating workflow..."
OUTPUT=$($CLI workflow create "test-pipeline")
WORKFLOW_ID=$(echo "$OUTPUT" | grep "ID:" | awk '{print $2}')
echo "$OUTPUT"
echo ""

# 2. 插入测试 agent 和 provider (绕过外键约束)
echo "2. Setting up test agents..."
sqlite3 "$DB_PATH" "INSERT INTO llm_providers (id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce) VALUES ('test-provider', 'openai_compatible', 'Test Provider', 'https://api.test.com', '[\"test-model\"]', 'test-model', X'00', X'00');"

sqlite3 "$DB_PATH" "INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('fetcher', 'Fetcher Agent', 'test-provider', 'You are a fetcher.');"
sqlite3 "$DB_PATH" "INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('validator', 'Validator Agent', 'test-provider', 'You are a validator.');"
sqlite3 "$DB_PATH" "INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('builder', 'Builder Agent', 'test-provider', 'You are a builder.');"
sqlite3 "$DB_PATH" "INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('tester', 'Tester Agent', 'test-provider', 'You are a tester.');"
sqlite3 "$DB_PATH" "INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('deployer', 'Deployer Agent', 'test-provider', 'You are a deployer.');"
echo "Done."
echo ""

# 3. 添加 jobs (新语法: --workflow 替代 --stage, 添加 --prompt)
echo "3. Adding jobs..."

# Job 1: fetcher (无依赖)
JOB1_OUTPUT=$($CLI workflow add-job --workflow "$WORKFLOW_ID" --agent "fetcher" --prompt "Fetch data from source" "fetch-data")
JOB1_ID=$(echo "$JOB1_OUTPUT" | grep "ID:" | awk '{print $2}')
echo "$JOB1_OUTPUT"

# Job 2: validator (无依赖)
JOB2_OUTPUT=$($CLI workflow add-job --workflow "$WORKFLOW_ID" --agent "validator" --prompt "Validate input data" "validate-input")
JOB2_ID=$(echo "$JOB2_OUTPUT" | grep "ID:" | awk '{print $2}')
echo "$JOB2_OUTPUT"

# Job 3: builder (依赖 job1)
JOB3_OUTPUT=$($CLI workflow add-job --workflow "$WORKFLOW_ID" --agent "builder" --prompt "Build the project" --depends-on "$JOB1_ID" "compile")
JOB3_ID=$(echo "$JOB3_OUTPUT" | grep "ID:" | awk '{print $2}')
echo "$JOB3_OUTPUT"

# Job 4: tester (依赖 job3)
JOB4_OUTPUT=$($CLI workflow add-job --workflow "$WORKFLOW_ID" --agent "tester" --prompt "Run tests" --depends-on "$JOB3_ID" "run-tests")
JOB4_ID=$(echo "$JOB4_OUTPUT" | grep "ID:" | awk '{print $2}')
echo "$JOB4_OUTPUT"

# Job 5: deployer (依赖 job4)
JOB5_OUTPUT=$($CLI workflow add-job --workflow "$WORKFLOW_ID" --agent "deployer" --prompt "Deploy artifacts" --depends-on "$JOB4_ID" "push-artifacts")
echo "$JOB5_OUTPUT"
echo ""

# 4. 列出所有 workflows
echo "4. Listing all workflows..."
$CLI workflow list
echo ""

# 5. 查看状态 (树形输出)
echo "5. Workflow status (tree view):"
echo "────────────────────────────────────────"
$CLI workflow status "$WORKFLOW_ID"
echo "────────────────────────────────────────"
echo ""

# 6. 更新部分 job 状态
echo "6. Updating job statuses (simulating execution)..."
$CLI workflow job-status --id "$JOB1_ID" succeeded
$CLI workflow job-status --id "$JOB2_ID" running
echo ""

# 7. 查看更新后的状态
echo "7. Status after updates:"
echo "────────────────────────────────────────"
$CLI workflow status "$WORKFLOW_ID"
echo "────────────────────────────────────────"
echo ""

# 8. 测试状态回退保护 (succeeded -> running 应该失败)
echo "8. Testing status rollback protection..."
echo "Attempt: succeeded -> running (should fail)..."
if $CLI workflow job-status --id "$JOB1_ID" running 2>&1; then
    echo "ERROR: Should have failed!"
    exit 1
else
    echo "✓ Correctly rejected invalid transition"
fi
echo ""

# 9. 测试 failed 状态
echo "9. Testing failed status..."
$CLI workflow job-status --id "$JOB3_ID" running
$CLI workflow job-status --id "$JOB3_ID" failed
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
