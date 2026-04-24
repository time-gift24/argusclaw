<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";

import { getApiClient, type LlmProviderRecord } from "@/lib/api";
import { TinyButton, TinyTag } from "@/lib/opentiny";

const api = getApiClient();
const router = useRouter();

const providers = ref<LlmProviderRecord[]>([]);
const loading = ref(true);
const error = ref("");
const actionMessage = ref("");
const testingProviderId = ref<number | null>(null);
const deletingProviderId = ref<number | null>(null);

async function loadProviders() {
  loading.value = true;
  error.value = "";

  try {
    providers.value = await api.listProviders();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "加载提供方失败。";
  } finally {
    loading.value = false;
  }
}

function goToCreate() {
  router.push("/providers/new");
}

function editProvider(provider: LlmProviderRecord) {
  router.push(`/providers/${provider.id}/edit`);
}

async function testProvider(provider: LlmProviderRecord) {
  if (!api.testProvider) {
    error.value = "当前 API 客户端不支持提供方连接测试。";
    return;
  }

  testingProviderId.value = provider.id;
  error.value = "";
  actionMessage.value = "";

  try {
    const result = await api.testProvider(provider.id, provider.default_model);
    actionMessage.value = `连接测试：${result.message}`;
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "连接测试失败。";
  } finally {
    testingProviderId.value = null;
  }
}

async function deleteProvider(provider: LlmProviderRecord) {
  if (!api.deleteProvider) {
    error.value = "当前 API 客户端不支持删除提供方。";
    return;
  }

  deletingProviderId.value = provider.id;
  error.value = "";
  actionMessage.value = "";

  try {
    await api.deleteProvider(provider.id);
    actionMessage.value = "提供方已删除。";
    await loadProviders();
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "删除提供方失败。";
  } finally {
    deletingProviderId.value = null;
  }
}

onMounted(() => {
  void loadProviders();
});
</script>

<template>
  <div class="page-container">
    <article class="list-panel">
      <div class="panel-header">
        <div class="panel-header-left">
          <h3 class="panel-title">已配置提供方</h3>
          <TinyTag
            v-if="loading"
            type="info"
          >
            加载中
          </TinyTag>
          <TinyTag
            v-else
            type="success"
          >
            {{ providers.length }} 项
          </TinyTag>
        </div>
        <div class="panel-header-right">
          <TinyButton type="primary" @click="goToCreate">
            新增提供方
          </TinyButton>
        </div>
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
        v-if="providers.length === 0 && !loading"
        class="empty-state"
      >
        <p>暂无已配置的提供方</p>
        <TinyButton type="primary" @click="goToCreate">立即创建</TinyButton>
      </div>

      <div
        v-else
        class="provider-list"
      >
        <div
          v-for="provider in providers"
          :key="provider.id"
          class="provider-card"
        >
          <div class="provider-info">
            <div class="provider-header">
              <strong class="provider-name">{{ provider.display_name }}</strong>
              <TinyTag
                v-if="provider.is_default"
                type="success"
              >
                默认
              </TinyTag>
            </div>
            <span class="provider-url">{{ provider.base_url }}</span>
            <span class="provider-model">默认模型：{{ provider.default_model }}</span>
          </div>
          <div class="provider-actions">
            <TinyButton
              :data-testid="`test-provider-${provider.id}`"
              type="default"
              :disabled="testingProviderId === provider.id"
              @click="testProvider(provider)"
            >
              {{ testingProviderId === provider.id ? "测试中" : "测试连接" }}
            </TinyButton>
            <TinyButton
              :data-testid="`edit-provider-${provider.id}`"
              type="default"
              @click="editProvider(provider)"
            >
              编辑
            </TinyButton>
            <TinyButton
              :data-testid="`delete-provider-${provider.id}`"
              type="default"
              :disabled="deletingProviderId === provider.id"
              @click="deleteProvider(provider)"
            >
              {{ deletingProviderId === provider.id ? "删除中" : "删除" }}
            </TinyButton>
          </div>
        </div>
      </div>
    </article>
  </div>
</template>

<style scoped>
.page-container {
  width: 100%;
}

.list-panel {
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
  padding: var(--space-5);
  display: grid;
  gap: var(--space-4);
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
}

.panel-header-left {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.panel-title {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
}

.provider-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.provider-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--space-4);
  padding: var(--space-4);
  background: var(--surface-raised);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  transition: border-color var(--transition-base);
}

.provider-card:hover {
  border-color: var(--border-default);
}

.provider-info {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  flex: 1 1 320px;
  min-width: 0;
}

.provider-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.provider-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.provider-name {
  font-size: var(--text-sm);
  font-weight: 590;
  color: var(--text-primary);
}

.provider-url {
  font-size: var(--text-xs);
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.provider-model {
  font-size: var(--text-xs);
  color: var(--text-placeholder);
}

.empty-state {
  padding: var(--space-8) var(--space-4);
  text-align: center;
  color: var(--text-muted);
  font-size: var(--text-sm);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-4);
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
</style>
