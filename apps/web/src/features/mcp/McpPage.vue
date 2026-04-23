<script setup lang="ts">
import { onMounted, ref } from "vue";

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
              :type="server.enabled ? 'success' : 'danger'"
            >
              {{ server.status }}
            </TinyTag>
          </div>
          <span class="server-transport">{{ server.transport.kind }} 传输</span>
          <span class="server-tools">{{ server.discovered_tool_count }} 个工具</span>
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
          class="tool-list"
        >
          <span
            v-for="tool in toolsByServer[server.id]"
            :key="tool.tool_name_original"
            class="tool-pill"
          >
            {{ tool.tool_name_original }}
          </span>
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
}

.server-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.tool-list {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  flex: 1 1 100%;
  padding-top: var(--space-2);
}

.tool-pill {
  padding: var(--space-1) var(--space-2);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-sm);
  color: var(--text-muted);
  font-size: var(--text-xs);
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
</style>
