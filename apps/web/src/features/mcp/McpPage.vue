<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import { getApiClient, type McpDiscoveredToolRecord, type McpServerRecord } from "@/lib/api";
import {
  TinyButton,
  TinyForm,
  TinyFormItem,
  TinyInput,
  TinyNumeric,
  TinyOption,
  TinySelect,
  TinySwitch,
  TinyTag,
} from "@/lib/opentiny";

const api = getApiClient();
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

const servers = ref<McpServerRecord[]>([]);
const loading = ref(true);
const saving = ref(false);
const error = ref("");
const actionMessage = ref("");
const deletingServerId = ref<number | null>(null);
const testingServerId = ref<number | null>(null);
const testingDraft = ref(false);
const loadingToolsServerId = ref<number | null>(null);
const toolsByServer = ref<Record<number, McpDiscoveredToolRecord[]>>({});
const importJsonText = ref("");
const importingConfig = ref(false);
const creationMode = ref("manual");

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
const isEditing = computed(() => form.value.id !== null);
const submitLabel = computed(() => {
  if (saving.value) {
    return isEditing.value ? "更新中…" : "创建中…";
  }

  return isEditing.value ? "更新 MCP 服务" : "创建 MCP 服务";
});

const summary = computed(() => {
  return {
    total: servers.value.length,
    ready: servers.value.filter((server) => server.enabled && server.status === "ready").length,
    attention: servers.value.filter((server) => !server.enabled || ["failed", "retrying"].includes(server.status)).length,
    tools: servers.value.reduce((count, server) => count + server.discovered_tool_count, 0),
  };
});

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
      if (separator <= 0) {
        return result;
      }

      const key = entry.slice(0, separator).trim();
      const value = entry.slice(separator + 1).trim();
      if (key) {
        result[key] = value;
      }
      return result;
    }, {});
}

function parseListLines(input: string) {
  return input
    .split("\n")
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function asObject(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  return value as Record<string, unknown>;
}

function parseJsonStringArray(value: unknown, serverName: string, fieldName: string) {
  if (value === undefined) {
    return [];
  }
  if (!Array.isArray(value)) {
    throw new Error(`${serverName} 的 ${fieldName} 必须是字符串数组。`);
  }

  return value.map((entry) => {
    if (typeof entry !== "string") {
      throw new Error(`${serverName} 的 ${fieldName} 只能包含字符串。`);
    }
    return entry;
  });
}

function parseJsonStringMap(value: unknown, serverName: string, fieldName: string) {
  if (value === undefined) {
    return {};
  }

  const objectValue = asObject(value);
  if (!objectValue) {
    throw new Error(`${serverName} 的 ${fieldName} 必须是对象。`);
  }

  return Object.entries(objectValue).reduce<Record<string, string>>((result, [key, entry]) => {
    if (typeof entry !== "string") {
      throw new Error(`${serverName} 的 ${fieldName}.${key} 必须是字符串。`);
    }
    result[key] = entry;
    return result;
  }, {});
}

function importedRecordsFromJson(input: string): McpServerRecord[] {
  if (!input.trim()) {
    throw new Error("请粘贴 MCP JSON 配置。");
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(input);
  } catch {
    throw new Error("MCP JSON 格式无效，请检查逗号、引号和括号。");
  }

  const root = asObject(parsed);
  if (!root) {
    throw new Error("MCP JSON 顶层必须是对象。");
  }

  const serversRoot = asObject(root.mcpServers) ?? root;
  const entries = Object.entries(serversRoot);
  if (entries.length === 0) {
    throw new Error("MCP JSON 中没有可导入的服务。");
  }

  return entries.map(([displayName, config]) => {
    const normalizedName = displayName.trim();
    if (!normalizedName) {
      throw new Error("MCP 服务名称不能为空。");
    }

    const serverConfig = asObject(config);
    if (!serverConfig) {
      throw new Error(`${normalizedName} 的配置必须是对象。`);
    }
    if (typeof serverConfig.command !== "string" || !serverConfig.command.trim()) {
      throw new Error(`${normalizedName} 缺少 command。`);
    }

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
  if (!record.display_name) {
    return "请填写 MCP 服务名称。";
  }

  if (record.transport.kind === "stdio" && !record.transport.command) {
    return "请填写 stdio 启动命令。";
  }

  if ((record.transport.kind === "http" || record.transport.kind === "sse") && !record.transport.url) {
    return "请填写服务地址。";
  }

  return "";
}

function resetForm() {
  form.value = createFormState();
}

function editMcpServer(server: McpServerRecord) {
  creationMode.value = "manual";
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
    return;
  }

  form.value = createFormState({
    ...baseState,
    transport_kind: server.transport.kind,
    url: server.transport.url,
    headersText: stringifyKeyValueMap(server.transport.headers),
  });
}

function updateFormField<K extends keyof McpFormState>(key: K, value: McpFormState[K]) {
  form.value = {
    ...form.value,
    [key]: value,
  };
}

function updateTransportKind(value: string | number) {
  updateFormField("transport_kind", value as McpTransportKind);
}

function updateTimeout(value: string | number | null) {
  const nextValue = Number(value);
  updateFormField("timeout_ms", Number.isFinite(nextValue) && nextValue > 0 ? nextValue : 5000);
}

function transportLabel(server: McpServerRecord) {
  if (server.transport.kind === "stdio") {
    return `stdio：${server.transport.command}`;
  }

  return `${server.transport.kind}：${server.transport.url}`;
}

function statusType(server: McpServerRecord) {
  if (!server.enabled) {
    return "danger";
  }
  return server.status === "ready" ? "success" : server.status === "failed" ? "danger" : "warning";
}

function formatNullable(value: string | null) {
  return value ?? "暂无";
}

function schemaPreview(tool: McpDiscoveredToolRecord) {
  return JSON.stringify(tool.schema, null, 2);
}

async function saveMcpServerDraft() {
  const record = recordFromForm();
  const validationError = validateRecord(record);
  if (validationError) {
    error.value = validationError;
    return;
  }

  saving.value = true;
  error.value = "";
  actionMessage.value = "";

  try {
    await api.saveMcpServer(record);
    actionMessage.value = isEditing.value ? "MCP 服务已更新。" : "MCP 服务已创建。";
    resetForm();
    await loadServers();
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

async function loadServers() {
  loading.value = true;
  error.value = "";

  try {
    servers.value = await api.listMcpServers();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载 MCP 服务失败。";
  } finally {
    loading.value = false;
  }
}

async function importMcpJson() {
  let records: McpServerRecord[];
  try {
    records = importedRecordsFromJson(importJsonText.value);
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "解析 MCP JSON 失败。";
    actionMessage.value = "";
    return;
  }

  importingConfig.value = true;
  error.value = "";
  actionMessage.value = "";

  try {
    for (const record of records) {
      await api.saveMcpServer(record);
    }
    actionMessage.value = `已导入 ${records.length} 个 MCP 服务。`;
    importJsonText.value = "";
    await loadServers();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "导入 MCP JSON 失败。";
  } finally {
    importingConfig.value = false;
  }
}

async function loadServerTools(server: McpServerRecord) {
  if (!server.id) {
    return;
  }
  if (!api.listMcpServerTools) {
    error.value = "当前 API 客户端不支持读取 MCP 工具。";
    return;
  }

  loadingToolsServerId.value = server.id;
  error.value = "";
  actionMessage.value = "";

  try {
    toolsByServer.value = {
      ...toolsByServer.value,
      [server.id]: await api.listMcpServerTools(server.id),
    };
    actionMessage.value = "MCP 工具列表已刷新。";
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "读取 MCP 工具失败。";
  } finally {
    loadingToolsServerId.value = null;
  }
}

async function testMcpServer(server: McpServerRecord) {
  if (!server.id) {
    return;
  }
  if (!api.testMcpServer) {
    error.value = "当前 API 客户端不支持 MCP 连接测试。";
    return;
  }

  testingServerId.value = server.id;
  error.value = "";
  actionMessage.value = "";

  try {
    const result = await api.testMcpServer(server.id);
    actionMessage.value = `连接测试：${result.message}`;
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "MCP 连接测试失败。";
  } finally {
    testingServerId.value = null;
  }
}

async function deleteMcpServer(server: McpServerRecord) {
  if (!server.id) {
    return;
  }
  if (!api.deleteMcpServer) {
    error.value = "当前 API 客户端不支持删除 MCP 服务。";
    return;
  }

  deletingServerId.value = server.id;
  error.value = "";
  actionMessage.value = "";

  try {
    await api.deleteMcpServer(server.id);
    actionMessage.value = "MCP 服务已删除。";
    if (form.value.id === server.id) {
      resetForm();
    }
    await loadServers();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "删除 MCP 服务失败。";
  } finally {
    deletingServerId.value = null;
  }
}

onMounted(() => {
  void loadServers();
});
</script>

<template>
  <section class="page-section">
    <div class="page-header">
      <div class="page-header-left">
        <h3 class="page-title">MCP 服务</h3>
        <TinyTag v-if="!loading">
          {{ servers.length }} 项
        </TinyTag>
      </div>
      <TinyButton
        data-testid="refresh-mcp"
        type="default"
        :disabled="loading"
        @click="loadServers"
      >
        {{ loading ? "刷新中" : "刷新" }}
      </TinyButton>
    </div>

    <div
      v-if="loading"
      class="loading-state"
    >
      加载中...
    </div>

    <p
      v-if="error"
      class="error-message"
    >
      {{ error }}
    </p>

    <p
      v-if="actionMessage"
      class="success-message"
    >
      {{ actionMessage }}
    </p>

    <div
      v-if="!loading"
      class="ops-grid"
    >
      <article class="ops-card">
        <span class="ops-label">总服务</span>
        <strong class="ops-value">{{ summary.total }}</strong>
      </article>
      <article class="ops-card">
        <span class="ops-label">就绪服务</span>
        <strong class="ops-value">{{ summary.ready }}</strong>
      </article>
      <article class="ops-card">
        <span class="ops-label">需关注</span>
        <strong class="ops-value">{{ summary.attention }}</strong>
      </article>
      <article class="ops-card">
        <span class="ops-label">已发现工具</span>
        <strong class="ops-value">{{ summary.tools }}</strong>
      </article>
    </div>

    <div
      v-if="!loading && servers.length === 0"
      class="empty-state"
    >
      <p>暂无已配置的 MCP 服务</p>
    </div>

    <div
      v-if="!loading && servers.length > 0"
      class="server-list"
    >
      <article
        v-for="server in servers"
        :key="server.id ?? server.display_name"
        class="server-card"
      >
        <div class="server-info">
          <div class="server-header">
            <strong class="server-name">{{ server.display_name }}</strong>
            <TinyTag
              :type="statusType(server)"
            >
              {{ server.status }}
            </TinyTag>
          </div>
          <span class="server-transport">{{ transportLabel(server) }}</span>
          <span class="server-tools">{{ server.discovered_tool_count }} 个工具</span>
        </div>
        <div class="server-diagnostics">
          <span>超时：{{ server.timeout_ms }} ms</span>
          <span>最近检查：{{ formatNullable(server.last_checked_at) }}</span>
          <span>最近成功：{{ formatNullable(server.last_success_at) }}</span>
          <span
            v-if="server.last_error"
            class="diagnostic-error"
          >
            最近错误：{{ server.last_error }}
          </span>
        </div>
        <div class="server-actions">
          <TinyButton
            :data-testid="`tools-mcp-${server.id}`"
            type="default"
            :disabled="loadingToolsServerId === server.id"
            @click="loadServerTools(server)"
          >
            {{ loadingToolsServerId === server.id ? "读取中" : "查看工具" }}
          </TinyButton>
          <TinyButton
            :data-testid="`test-mcp-${server.id}`"
            type="default"
            :disabled="testingServerId === server.id"
            @click="testMcpServer(server)"
          >
            {{ testingServerId === server.id ? "测试中" : "测试连接" }}
          </TinyButton>
          <TinyButton
            :data-testid="`edit-mcp-${server.id}`"
            type="default"
            @click="editMcpServer(server)"
          >
            编辑
          </TinyButton>
          <TinyButton
            :data-testid="`delete-mcp-${server.id}`"
            type="default"
            :disabled="deletingServerId === server.id"
            @click="deleteMcpServer(server)"
          >
            {{ deletingServerId === server.id ? "删除中" : "删除" }}
          </TinyButton>
        </div>
        <div
          v-if="server.id && toolsByServer[server.id]"
          class="tool-panel"
        >
          <div class="tool-panel-header">
            <strong>已发现工具</strong>
            <TinyTag type="info">{{ toolsByServer[server.id].length }} 个</TinyTag>
          </div>
          <div
            v-if="toolsByServer[server.id].length === 0"
            class="tool-empty"
          >
            暂无已发现工具
          </div>
          <article
            v-for="tool in toolsByServer[server.id]"
            :key="tool.tool_name_original"
            class="tool-card"
          >
            <div class="tool-card-header">
              <strong>{{ tool.tool_name_original }}</strong>
            </div>
            <p>{{ tool.description || "暂无描述" }}</p>
            <details>
              <summary>Schema</summary>
              <pre>{{ schemaPreview(tool) }}</pre>
            </details>
          </article>
        </div>
      </article>
    </div>

    <article
      data-testid="mcp-create-card"
      class="form-panel create-panel"
    >
      <div class="panel-header">
        <h3 class="panel-title">{{ isEditing ? "编辑 MCP 服务" : "新增 MCP 服务" }}</h3>
        <p class="panel-description">
          {{ isEditing ? "更新已配置服务的连接参数" : "选择手动配置或 JSON 导入来创建 MCP 服务" }}
        </p>
      </div>

      <div
        class="creation-tabs"
        :class="`creation-tabs--${creationMode}`"
        role="tablist"
        aria-label="MCP 创建方式"
      >
        <TinyButton
          data-testid="mcp-create-tab-manual"
          role="tab"
          :aria-selected="creationMode === 'manual'"
          type="default"
          @click="creationMode = 'manual'"
        >
          手动配置
        </TinyButton>
        <TinyButton
          data-testid="mcp-create-tab-json"
          role="tab"
          :aria-selected="creationMode === 'json'"
          type="default"
          @click="creationMode = 'json'"
        >
          JSON 导入
        </TinyButton>
      </div>

      <Transition
        name="creation-panel"
        mode="out-in"
      >
        <div
          v-if="creationMode === 'manual'"
          key="manual"
          class="creation-tab-panel"
          role="tabpanel"
        >
          <form
            data-testid="mcp-form"
            class="mcp-form"
            @submit.prevent="saveMcpServerDraft"
          >
            <TinyForm
              label-position="top"
              class="mcp-form__grid"
            >
              <TinyFormItem label="服务名称">
                <TinyInput
                  :model-value="form.display_name"
                  name="mcp-display-name"
                  placeholder="例如：Docs MCP"
                  @update:model-value="updateFormField('display_name', String($event))"
                />
              </TinyFormItem>

              <TinyFormItem label="传输类型">
                <TinySelect
                  :model-value="form.transport_kind"
                  name="mcp-transport-kind"
                  @update:model-value="updateTransportKind"
                >
                  <TinyOption
                    label="stdio"
                    value="stdio"
                  />
                  <TinyOption
                    label="HTTP"
                    value="http"
                  />
                  <TinyOption
                    label="SSE"
                    value="sse"
                  />
                </TinySelect>
              </TinyFormItem>

              <template v-if="form.transport_kind === 'stdio'">
                <TinyFormItem label="启动命令">
                  <TinyInput
                    :model-value="form.command"
                    name="mcp-command"
                    placeholder="docs-mcp"
                    @update:model-value="updateFormField('command', String($event))"
                  />
                </TinyFormItem>

                <TinyFormItem label="参数列表">
                  <TinyInput
                    :model-value="form.argsText"
                    name="mcp-args"
                    type="textarea"
                    :rows="3"
                    placeholder="每行一个参数，例如 --stdio"
                    @update:model-value="updateFormField('argsText', String($event))"
                  />
                </TinyFormItem>

                <TinyFormItem
                  label="环境变量"
                  class="full-width"
                >
                  <TinyInput
                    :model-value="form.envText"
                    name="mcp-env"
                    type="textarea"
                    :rows="3"
                    placeholder="每行一个 KEY=value"
                    @update:model-value="updateFormField('envText', String($event))"
                  />
                </TinyFormItem>
              </template>

              <template v-else>
                <TinyFormItem
                  label="服务地址"
                  class="full-width"
                >
                  <TinyInput
                    :model-value="form.url"
                    name="mcp-url"
                    placeholder="https://example.com/mcp"
                    @update:model-value="updateFormField('url', String($event))"
                  />
                </TinyFormItem>

                <TinyFormItem
                  label="请求头"
                  class="full-width"
                >
                  <TinyInput
                    :model-value="form.headersText"
                    name="mcp-headers"
                    type="textarea"
                    :rows="3"
                    placeholder="每行一个 Header=Value"
                    @update:model-value="updateFormField('headersText', String($event))"
                  />
                </TinyFormItem>
              </template>

              <TinyFormItem label="连接超时">
                <TinyNumeric
                  :model-value="form.timeout_ms"
                  name="mcp-timeout"
                  :min="1000"
                  :step="1000"
                  @update:model-value="updateTimeout"
                />
              </TinyFormItem>

              <TinyFormItem label="启用服务">
                <div class="mcp-form__switch">
                  <TinySwitch
                    :model-value="form.enabled"
                    name="mcp-enabled"
                    @update:model-value="updateFormField('enabled', Boolean($event))"
                  />
                  <span class="switch-hint">保存后参与运行时连接与工具发现</span>
                </div>
              </TinyFormItem>
            </TinyForm>

            <div class="mcp-form__actions">
              <TinyButton
                native-type="submit"
                type="primary"
                :disabled="saving"
              >
                {{ submitLabel }}
              </TinyButton>
              <TinyButton
                data-testid="test-mcp-draft"
                type="default"
                :disabled="testingDraft"
                @click="testDraftMcpServer"
              >
                {{ testingDraft ? "测试中" : "测试当前配置" }}
              </TinyButton>
              <TinyButton
                type="default"
                @click="resetForm"
              >
                {{ isEditing ? "取消编辑" : "重置" }}
              </TinyButton>
            </div>
          </form>
        </div>

        <div
          v-else
          key="json"
          class="creation-tab-panel import-tab-content"
          role="tabpanel"
        >
          <p class="panel-description">
            支持 Claude / MCP 常见配置片段，例如 { "brave-search": { "command": "npx", "args": ["-y", "..."], "env": { "KEY": "xxx" } } }。
          </p>

          <TinyInput
            :model-value="importJsonText"
            name="mcp-import-json"
            type="textarea"
            :rows="8"
            placeholder='{ "brave-search": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-brave-search"], "env": { "BRAVE_API_KEY": "xxx" } } }'
            @update:model-value="importJsonText = String($event)"
          />

          <div class="import-actions">
            <TinyButton
              data-testid="import-mcp-json"
              type="primary"
              :disabled="importingConfig"
              @click="importMcpJson"
            >
              {{ importingConfig ? "导入中" : "导入配置" }}
            </TinyButton>
            <span class="import-hint">导入后会保存为 stdio MCP 服务，并使用默认 5000ms 超时。</span>
          </div>
        </div>
      </Transition>
    </article>
  </section>
</template>

<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
}

.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
}

.page-header-left {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.page-title {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
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

.mcp-form {
  display: grid;
  gap: var(--space-5);
}

.mcp-form__grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.mcp-form__grid :deep(.tiny-form-item) {
  margin-bottom: 0;
}

.mcp-form__switch,
.mcp-form__actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-3);
}

.switch-hint {
  font-size: var(--text-sm);
  color: var(--text-muted);
}

.create-panel,
.creation-tab-panel,
.import-tab-content {
  gap: var(--space-4);
}

.create-panel,
.creation-tab-panel,
.import-tab-content {
  display: grid;
}

.creation-tabs {
  position: relative;
  display: inline-grid;
  grid-template-columns: repeat(2, minmax(88px, 1fr));
  align-items: center;
  width: max-content;
  padding: 3px;
  overflow: hidden;
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-full);
}

.creation-tabs::before {
  position: absolute;
  inset: 3px auto 3px 3px;
  width: calc((100% - 6px) / 2);
  content: "";
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-full);
  box-shadow: var(--shadow-xs);
  pointer-events: none;
  transition:
    transform 180ms ease,
    box-shadow 180ms ease;
}

.creation-tabs--json::before {
  transform: translateX(100%);
}

.creation-tabs :deep(.tiny-button) {
  position: relative;
  z-index: 1;
  min-width: 88px;
  height: 30px;
  padding: 0 var(--space-3);
  font-size: var(--text-xs);
  font-weight: 560;
  color: var(--text-muted);
  background: transparent;
  border-color: transparent;
  box-shadow: none;
  transition:
    color 160ms ease,
    transform 160ms ease,
    opacity 160ms ease;
}

.creation-tabs :deep(.tiny-button[aria-selected="true"]) {
  color: var(--text-primary);
}

.creation-tabs :deep(.tiny-button:hover) {
  color: var(--text-primary);
  transform: translateY(-1px);
}

.creation-panel-enter-active,
.creation-panel-leave-active {
  transition:
    opacity 160ms ease,
    transform 160ms ease;
}

.creation-panel-enter-from,
.creation-panel-leave-to {
  opacity: 0;
  transform: translateY(6px);
}

.import-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-3);
}

.import-hint {
  color: var(--text-muted);
  font-size: var(--text-sm);
}

.full-width {
  grid-column: 1 / -1;
}

.ops-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--space-3);
}

.ops-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-4);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.ops-label {
  font-size: var(--text-xs);
  color: var(--text-muted);
}

.ops-value {
  font-size: var(--text-xl);
  color: var(--text-primary);
}

.server-list {
  display: grid;
  gap: var(--space-3);
}

.server-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--space-4);
  padding: var(--space-4) var(--space-5);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
  transition:
    border-color var(--transition-base),
    transform var(--transition-fast);
}

.server-card:hover {
  border-color: var(--border-strong);
}

.server-card:active {
  transform: scale(0.99);
}

.server-info {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  flex: 1 1 260px;
}

.server-diagnostics {
  display: grid;
  gap: var(--space-1);
  flex: 1 1 320px;
  color: var(--text-muted);
  font-size: var(--text-xs);
}

.diagnostic-error {
  color: var(--danger);
}

.server-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.tool-panel {
  display: grid;
  gap: var(--space-3);
  flex: 1 1 100%;
  padding-top: var(--space-2);
}

.tool-panel-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-primary);
  font-size: var(--text-sm);
}

.tool-card,
.tool-empty {
  padding: var(--space-3);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  color: var(--text-muted);
}

.tool-card {
  display: grid;
  gap: var(--space-2);
}

.tool-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: var(--text-primary);
  font-size: var(--text-xs);
}

.tool-card p {
  margin: 0;
  font-size: var(--text-xs);
  line-height: 1.5;
}

.tool-card details {
  font-size: var(--text-xs);
}

.tool-card summary {
  cursor: pointer;
  color: var(--accent);
}

.tool-card pre {
  overflow: auto;
  margin: var(--space-2) 0 0;
  padding: var(--space-2);
  background: var(--surface-base);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-sm);
}

.server-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.server-name {
  font-size: var(--text-sm);
  font-weight: 590;
  color: var(--text-primary);
}

.server-transport,
.server-tools {
  font-size: var(--text-xs);
  color: var(--text-muted);
}

.loading-state,
.empty-state {
  padding: var(--space-10) var(--space-4);
  text-align: center;
  color: var(--text-muted);
  font-size: var(--text-sm);
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
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

@media (max-width: 960px) {
  .ops-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .mcp-form__grid {
    grid-template-columns: 1fr;
  }
}
</style>
