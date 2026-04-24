<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";

import { getApiClient, type McpServerRecord, type McpConnectionTestResult } from "@/lib/api";
import { TinyButton, TinyInput } from "@/lib/opentiny";

const api = getApiClient();
const router = useRouter();

const importJsonText = ref("");
const importingConfig = ref(false);
const testingDraft = ref(false);
const error = ref("");
const actionMessage = ref("");

function asObject(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}

function parseJsonStringArray(value: unknown, serverName: string, fieldName: string) {
  if (value === undefined) return [];
  if (!Array.isArray(value)) throw new Error(`${serverName} 的 ${fieldName} 必须是字符串数组。`);
  return value.map((entry) => {
    if (typeof entry !== "string") throw new Error(`${serverName} 的 ${fieldName} 只能包含字符串。`);
    return entry;
  });
}

function parseJsonStringMap(value: unknown, serverName: string, fieldName: string) {
  if (value === undefined) return {};
  const objectValue = asObject(value);
  if (!objectValue) throw new Error(`${serverName} 的 ${fieldName} 必须是对象。`);
  return Object.entries(objectValue).reduce<Record<string, string>>((result, [key, entry]) => {
    if (typeof entry !== "string") throw new Error(`${serverName} 的 ${fieldName}.${key} 必须是字符串。`);
    result[key] = entry;
    return result;
  }, {});
}

function importedRecordsFromJson(input: string): McpServerRecord[] {
  if (!input.trim()) throw new Error("请粘贴 MCP JSON 配置。");
  let parsed: unknown;
  try {
    parsed = JSON.parse(input);
  } catch {
    throw new Error("MCP JSON 格式无效，请检查逗号、引号和括号。");
  }

  const root = asObject(parsed);
  if (!root) throw new Error("MCP JSON 顶层必须是对象。");

  const serversRoot = asObject(root.mcpServers) ?? root;
  const entries = Object.entries(serversRoot);
  if (entries.length === 0) throw new Error("MCP JSON 中没有可导入的服务。");

  return entries.map(([displayName, config]) => {
    const normalizedName = displayName.trim();
    if (!normalizedName) throw new Error("MCP 服务名称不能为空。");
    const serverConfig = asObject(config);
    if (!serverConfig) throw new Error(`${normalizedName} 的配置必须是对象。`);
    if (typeof serverConfig.command !== "string" || !serverConfig.command.trim()) throw new Error(`${normalizedName} 缺少 command。`);

    return {
      id: null,
      display_name: normalizedName,
      enabled: true,
      transport: {
        kind: "stdio" as const,
        command: serverConfig.command.trim(),
        args: parseJsonStringArray(serverConfig.args, normalizedName, "args"),
        env: parseJsonStringMap(serverConfig.env, normalizedName, "env"),
      },
      timeout_ms: 5000,
      status: "connecting" as const,
      last_checked_at: null,
      last_success_at: null,
      last_error: null,
      discovered_tool_count: 0,
    };
  });
}

async function importMcpJson() {
  let records: McpServerRecord[];
  try {
    records = importedRecordsFromJson(importJsonText.value);
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "解析 MCP JSON 失败。";
    return;
  }

  importingConfig.value = true;
  error.value = "";

  try {
    for (const record of records) {
      await api.saveMcpServer(record);
    }
    router.push("/mcp");
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "导入 MCP JSON 失败。";
  } finally {
    importingConfig.value = false;
  }
}

async function testImportedMcpJson() {
  if (!api.testMcpServerDraft) {
    error.value = "当前 API 客户端不支持临时 MCP 配置测试。";
    return;
  }

  let records: McpServerRecord[];
  try {
    records = importedRecordsFromJson(importJsonText.value);
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "解析 MCP JSON 失败。";
    return;
  }

  testingDraft.value = true;
  error.value = "";
  actionMessage.value = "";

  try {
    const results: McpConnectionTestResult[] = [];
    for (const record of records) {
      results.push(await api.testMcpServerDraft(record));
    }
    const firstResult = results[0];
    if (results.length === 1 && firstResult) {
      actionMessage.value = `JSON 配置测试：${firstResult.message}`;
    } else {
      actionMessage.value = `已测试 ${results.length} 个 JSON MCP 配置。`;
    }
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "JSON MCP 配置测试失败。";
  } finally {
    testingDraft.value = false;
  }
}

function goBack() {
  router.push("/mcp");
}
</script>

<template>
  <div class="edit-page">
    <article class="form-panel">
      <div class="panel-header">
        <h3 class="panel-title">JSON 导入 MCP 服务</h3>
        <p class="panel-description">
          粘贴 MCP JSON 配置片段，批量导入 stdio 传输的服务。
        </p>
      </div>

      <div class="import-tab-content">
        <p class="panel-description">
          支持 Claude / MCP 常见配置片段，例如 { "brave-search": { "command": "npx", "args": ["-y", "..."], "env": { "KEY": "xxx" } } }。
        </p>

        <TinyInput
          v-model="importJsonText"
          type="textarea"
          :rows="12"
          placeholder='{ "brave-search": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-brave-search"], "env": { "BRAVE_API_KEY": "xxx" } } }'
        />

        <span class="import-hint">导入后会保存为 stdio MCP 服务，并使用默认 5000ms 超时。</span>
      </div>

      <div class="creation-action-bar">
        <TinyButton type="primary" :disabled="importingConfig" @click="importMcpJson">
          {{ importingConfig ? "导入中…" : "开始导入" }}
        </TinyButton>
        <TinyButton type="default" :disabled="testingDraft" @click="testImportedMcpJson">
          {{ testingDraft ? "测试中" : "测试 JSON 配置" }}
        </TinyButton>
        <TinyButton type="default" @click="goBack">取消</TinyButton>
      </div>

      <p v-if="error" class="error-message">{{ error }}</p>
      <p v-if="actionMessage" class="success-message">{{ actionMessage }}</p>
    </article>
  </div>
</template>

<style scoped>
.edit-page {
  max-width: 800px;
}

.form-panel {
  display: grid;
  gap: var(--space-5);
  padding: var(--space-5);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.panel-header {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.panel-title {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
}

.panel-description {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
}

.import-tab-content {
  display: grid;
  gap: var(--space-4);
}

.import-hint {
  color: var(--text-muted);
  font-size: var(--text-sm);
}

.creation-action-bar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-3);
  margin-top: var(--space-2);
}

.error-message,
.success-message {
  margin: 0;
  padding: var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
}

.error-message {
  background: var(--danger-bg);
  border: 1px solid var(--danger-border);
  color: var(--danger);
}

.success-message {
  background: var(--success-bg);
  border: 1px solid var(--success-border);
  color: var(--success);
}
</style>
