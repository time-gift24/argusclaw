<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";

import { getApiClient, type McpServerRecord } from "@/lib/api";
import {
  TinyButton,
  TinyForm,
  TinyFormItem,
  TinyInput,
  TinyNumeric,
  TinyOption,
  TinySelect,
  TinySwitch,
} from "@/lib/opentiny";

const api = getApiClient();
const route = useRoute();
const router = useRouter();

type McpTransportKind = McpServerRecord["transport"]["kind"];

interface McpFormState {
  id: number | null;
  display_name: string;
  enabled: boolean;
  transport_kind: McpTransportKind;
  command: string;
  argsText: string;
  envText: string;
  url: string;
  headersText: string;
  timeout_ms: number;
  status: McpServerRecord["status"];
  last_checked_at: string | null;
  last_success_at: string | null;
  last_error: string | null;
  discovered_tool_count: number;
}

function createFormState(overrides: Partial<McpFormState> = {}): McpFormState {
  return {
    id: null,
    display_name: "",
    enabled: true,
    transport_kind: "stdio",
    command: "",
    argsText: "",
    envText: "",
    url: "",
    headersText: "",
    timeout_ms: 5000,
    status: "connecting",
    last_checked_at: null,
    last_success_at: null,
    last_error: null,
    discovered_tool_count: 0,
    ...overrides,
  };
}

const form = ref<McpFormState>(createFormState());
const isEditing = ref(false);
const loading = ref(true);
const saving = ref(false);
const testingDraft = ref(false);
const error = ref("");
const actionMessage = ref("");

async function loadServer() {
  isEditing.value = !!route.params.serverId;
  if (!isEditing.value) {
    loading.value = false;
    return;
  }

  loading.value = true;
  error.value = "";

  try {
    const servers = await api.listMcpServers();
    const serverId = parseInt(route.params.serverId as string, 10);
    const server = servers.find(s => s.id === serverId);

    if (server) {
      const baseState = {
        id: server.id,
        display_name: server.display_name,
        enabled: server.enabled,
        timeout_ms: server.timeout_ms,
        status: server.status,
        last_checked_at: server.last_checked_at,
        last_success_at: server.last_success_at,
        last_error: server.last_error,
        discovered_tool_count: server.discovered_tool_count,
      };

      if (server.transport.kind === "stdio") {
        form.value = createFormState({
          ...baseState,
          transport_kind: "stdio",
          command: server.transport.command,
          argsText: server.transport.args.join("\n"),
          envText: stringifyKeyValueMap(server.transport.env),
        });
      } else {
        form.value = createFormState({
          ...baseState,
          transport_kind: server.transport.kind,
          url: server.transport.url,
          headersText: stringifyKeyValueMap(server.transport.headers),
        });
      }
    } else {
      error.value = "未找到该 MCP 服务。";
    }
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载 MCP 服务失败。";
  } finally {
    loading.value = false;
  }
}

function stringifyKeyValueMap(value: Record<string, string>) {
  return Object.entries(value)
    .map(([key, entry]) => `${key}=${entry}`)
    .join("\n");
}

function parseKeyValueLines(input: string) {
  return input
    .split("\n")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .reduce<Record<string, string>>((result, entry) => {
      const separator = entry.indexOf("=");
      if (separator <= 0) return result;
      const key = entry.slice(0, separator).trim();
      const value = entry.slice(separator + 1).trim();
      if (key) result[key] = value;
      return result;
    }, {});
}

function parseListLines(input: string) {
  return input
    .split("\n")
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function recordFromForm(): McpServerRecord {
  const current = form.value;
  const transport =
    current.transport_kind === "stdio"
      ? {
          kind: "stdio" as const,
          command: current.command.trim(),
          args: parseListLines(current.argsText),
          env: parseKeyValueLines(current.envText),
        }
      : {
          kind: current.transport_kind,
          url: current.url.trim(),
          headers: parseKeyValueLines(current.headersText),
        };

  return {
    id: current.id,
    display_name: current.display_name.trim(),
    enabled: current.enabled,
    transport,
    timeout_ms: current.timeout_ms,
    status: current.enabled ? current.status : "disabled",
    last_checked_at: current.last_checked_at,
    last_success_at: current.last_success_at,
    last_error: current.last_error,
    discovered_tool_count: current.discovered_tool_count,
  };
}

function validateRecord(record: McpServerRecord) {
  if (!record.display_name) return "请填写 MCP 服务名称。";
  if (record.transport.kind === "stdio" && !record.transport.command) return "请填写 stdio 启动命令。";
  if ((record.transport.kind === "http" || record.transport.kind === "sse") && !record.transport.url) return "请填写服务地址。";
  return "";
}

async function saveMcpServer() {
  const record = recordFromForm();
  const validationError = validateRecord(record);
  if (validationError) {
    error.value = validationError;
    return;
  }

  saving.value = true;
  error.value = "";

  try {
    await api.saveMcpServer(record);
    router.push("/mcp");
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "保存 MCP 服务失败。";
  } finally {
    saving.value = false;
  }
}

async function testDraftMcpServer() {
  if (!api.testMcpServerDraft) {
    error.value = "当前 API 客户端不支持临时 MCP 配置测试。";
    return;
  }

  const record = recordFromForm();
  const validationError = validateRecord(record);
  if (validationError) {
    error.value = validationError;
    return;
  }

  testingDraft.value = true;
  error.value = "";
  actionMessage.value = "";

  try {
    const result = await api.testMcpServerDraft(record);
    actionMessage.value = `当前配置测试：${result.message}`;
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "当前 MCP 配置测试失败。";
  } finally {
    testingDraft.value = false;
  }
}

function goBack() {
  router.push("/mcp");
}

onMounted(() => {
  void loadServer();
});

watch(
  () => route.params.serverId,
  () => {
    void loadServer();
  }
);
</script>

<template>
  <div class="edit-page">
    <article class="form-panel">
      <div class="panel-header">
        <h3 class="panel-title">{{ isEditing ? "编辑 MCP 服务" : "新增 MCP 服务" }}</h3>
        <p class="panel-description">
          {{ isEditing ? "更新已配置服务的连接参数" : "填写下方信息以添加新的 MCP 服务" }}
        </p>
      </div>

      <p v-if="loading" class="loading-message">加载中...</p>

      <template v-else>
        <form class="mcp-form" @submit.prevent="saveMcpServer">
          <TinyForm label-position="top" class="mcp-form__grid">
            <TinyFormItem label="服务名称">
              <TinyInput
                v-model="form.display_name"
                placeholder="例如：Docs MCP"
              />
            </TinyFormItem>

            <TinyFormItem label="传输类型">
              <TinySelect v-model="form.transport_kind">
                <TinyOption label="stdio" value="stdio" />
                <TinyOption label="HTTP" value="http" />
                <TinyOption label="SSE" value="sse" />
              </TinySelect>
            </TinyFormItem>

            <template v-if="form.transport_kind === 'stdio'">
              <TinyFormItem label="启动命令">
                <TinyInput v-model="form.command" placeholder="docs-mcp" />
              </TinyFormItem>

              <TinyFormItem label="参数列表">
                <TinyInput
                  v-model="form.argsText"
                  type="textarea"
                  :rows="3"
                  placeholder="每行一个参数，例如 --stdio"
                />
              </TinyFormItem>

              <TinyFormItem label="环境变量" class="full-width">
                <TinyInput
                  v-model="form.envText"
                  type="textarea"
                  :rows="3"
                  placeholder="每行一个 KEY=value"
                />
              </TinyFormItem>
            </template>

            <template v-else>
              <TinyFormItem label="服务地址" class="full-width">
                <TinyInput v-model="form.url" placeholder="https://example.com/mcp" />
              </TinyFormItem>

              <TinyFormItem label="请求头" class="full-width">
                <TinyInput
                  v-model="form.headersText"
                  type="textarea"
                  :rows="3"
                  placeholder="每行一个 Header=Value"
                />
              </TinyFormItem>
            </template>

            <TinyFormItem label="连接超时">
              <TinyNumeric v-model="form.timeout_ms" :min="1000" :step="1000" />
            </TinyFormItem>

            <TinyFormItem label="启用服务">
              <div class="mcp-form__switch">
                <TinySwitch v-model="form.enabled" />
                <span class="switch-hint">保存后参与运行时连接与工具发现</span>
              </div>
            </TinyFormItem>
          </TinyForm>
        </form>

        <div class="creation-action-bar">
          <TinyButton
            data-testid="save-mcp"
            type="primary"
            :disabled="saving"
            @click="saveMcpServer"
          >
            {{ saving ? "保存中" : (isEditing ? "更新 MCP 服务" : "创建 MCP 服务") }}
          </TinyButton>
          <TinyButton type="default" :disabled="testingDraft" @click="testDraftMcpServer">
            {{ testingDraft ? "测试中" : "测试当前配置" }}
          </TinyButton>
          <TinyButton type="default" @click="goBack">取消</TinyButton>
        </div>

        <p v-if="error" class="error-message">{{ error }}</p>
        <p v-if="actionMessage" class="success-message">{{ actionMessage }}</p>
      </template>
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

.mcp-form__grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.full-width {
  grid-column: 1 / -1;
}

.mcp-form__switch {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-3);
}

.switch-hint {
  font-size: var(--text-sm);
  color: var(--text-muted);
}

.creation-action-bar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-3);
  margin-top: var(--space-2);
}

.loading-message {
  text-align: center;
  color: var(--text-muted);
  padding: var(--space-5);
}

.error-message {
  margin: 0;
  padding: var(--space-3);
  background: var(--danger-bg);
  border: 1px solid var(--danger-border);
  border-radius: var(--radius-md);
  color: var(--danger);
  font-size: var(--text-sm);
}

.success-message {
  margin: 0;
  padding: var(--space-3);
  background: var(--success-bg);
  border: 1px solid var(--success-border);
  border-radius: var(--radius-md);
  color: var(--success);
  font-size: var(--text-sm);
}

@media (max-width: 960px) {
  .mcp-form__grid {
    grid-template-columns: 1fr;
  }
}
</style>
