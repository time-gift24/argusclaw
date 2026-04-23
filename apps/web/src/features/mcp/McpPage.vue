<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import { getApiClient, type McpDiscoveredToolRecord, type McpServerRecord } from "@/lib/api";
import { TinyButton, TinyTag } from "@/lib/opentiny";

const api = getApiClient();
const servers = ref<McpServerRecord[]>([]);
const loading = ref(true);
const error = ref("");
const actionMessage = ref("");
const deletingServerId = ref<number | null>(null);
const testingServerId = ref<number | null>(null);
const loadingToolsServerId = ref<number | null>(null);
const toolsByServer = ref<Record<number, McpDiscoveredToolRecord[]>>({});

const summary = computed(() => {
  return {
    total: servers.value.length,
    ready: servers.value.filter((server) => server.enabled && server.status === "ready").length,
    attention: servers.value.filter((server) => !server.enabled || ["failed", "retrying"].includes(server.status)).length,
    tools: servers.value.reduce((count, server) => count + server.discovered_tool_count, 0),
  };
});

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
}
</style>
