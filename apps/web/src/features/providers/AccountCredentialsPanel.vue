<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import { getApiClient, type AccountStatus } from "@/lib/api";
import { TinyButton, TinyForm, TinyFormItem, TinyInput } from "@/lib/opentiny";

const api = getApiClient();

const accountLoading = ref(false);
const accountSaving = ref(false);
const accountError = ref("");
const accountMessage = ref("");
const accountStatus = ref<AccountStatus>({
  configured: false,
  username: null,
});
const accountUsername = ref("");
const accountPassword = ref("");

const accountStatusText = computed(() => {
  if (accountLoading.value) {
    return "账号状态加载中...";
  }
  if (accountStatus.value.configured && accountStatus.value.username) {
    return `当前已配置账号：${accountStatus.value.username}`;
  }
  return "尚未配置服务端账号。";
});

function applyAccountStatus(status: AccountStatus) {
  accountStatus.value = status;
  if (!accountUsername.value.trim()) {
    accountUsername.value = status.username ?? "";
  }
}

async function loadAccountStatus() {
  if (!api.getAccountStatus) {
    return;
  }

  accountLoading.value = true;
  accountError.value = "";

  try {
    applyAccountStatus(await api.getAccountStatus());
  } catch (reason) {
    accountError.value = reason instanceof Error ? reason.message : "加载账号状态失败。";
  } finally {
    accountLoading.value = false;
  }
}

async function saveAccount() {
  if (!api.configureAccount) {
    accountError.value = "当前 API 客户端不支持账号配置。";
    return;
  }

  const username = accountUsername.value.trim();
  if (!username) {
    accountError.value = "请输入账号用户名。";
    return;
  }
  if (!accountPassword.value.trim()) {
    accountError.value = "请输入账号密码。";
    return;
  }

  accountSaving.value = true;
  accountError.value = "";
  accountMessage.value = "";

  try {
    const status = await api.configureAccount({
      username,
      password: accountPassword.value,
    });
    applyAccountStatus(status);
    accountPassword.value = "";
    accountMessage.value = `账号凭据已保存：${status.username ?? username}`;
  } catch (reason) {
    accountError.value = reason instanceof Error ? reason.message : "保存账号凭据失败。";
  } finally {
    accountSaving.value = false;
  }
}

onMounted(() => {
  void loadAccountStatus();
});
</script>

<template>
  <section
    class="account-panel"
    data-testid="account-panel"
  >
    <div class="account-panel__header">
      <h3 class="account-panel__title">服务端账号凭据</h3>
      <p class="account-panel__description">
        {{ accountStatusText }} 该凭据仅保存在服务端账号表中，用于运行时换取 provider token，不会写入提供方配置。
      </p>
    </div>

    <TinyForm
      label-position="top"
      class="account-panel__grid"
    >
      <TinyFormItem label="账号用户名">
        <TinyInput
          :model-value="accountUsername"
          name="account-username"
          placeholder="账号用户名"
          @update:model-value="accountUsername = String($event)"
        />
      </TinyFormItem>
      <TinyFormItem label="账号密码">
        <TinyInput
          :model-value="accountPassword"
          name="account-password"
          placeholder="账号密码"
          type="password"
          @update:model-value="accountPassword = String($event)"
        />
      </TinyFormItem>
    </TinyForm>

    <div class="account-panel__actions">
      <TinyButton
        data-testid="save-account"
        type="default"
        :disabled="accountSaving"
        @click="saveAccount"
      >
        {{ accountSaving ? "保存中..." : "保存账号凭据" }}
      </TinyButton>
    </div>

    <p
      v-if="accountError"
      class="error-message"
    >
      {{ accountError }}
    </p>
    <p
      v-if="accountMessage"
      class="success-message"
    >
      {{ accountMessage }}
    </p>
  </section>
</template>

<style scoped>
.account-panel {
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
  padding: var(--space-5);
  display: grid;
  gap: var(--space-4);
}

.account-panel__header {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.account-panel__title {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 590;
  color: var(--text-primary);
}

.account-panel__description {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
}

.account-panel__grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.account-panel__grid :deep(.tiny-form-item) {
  margin-bottom: 0;
}

.account-panel__actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
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
  .account-panel__grid {
    grid-template-columns: 1fr;
  }
}
</style>
