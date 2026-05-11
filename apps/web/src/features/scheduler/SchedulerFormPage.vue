<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";

import {
  getApiClient,
  type AgentRecord,
  type CreateScheduledMessageRequest,
  type LlmProviderRecord,
  type ScheduledMessageSummary,
} from "@/lib/api";
import { TinyButton, TinyInput, TinyOption, TinySelect } from "@/lib/opentiny";

type ScheduleMode = "cron" | "once";

const route = useRoute();
const router = useRouter();
const providers = ref<LlmProviderRecord[]>([]);
const templates = ref<AgentRecord[]>([]);
const loading = ref(true);
const submitting = ref(false);
const error = ref("");
const actionMessage = ref("");
const scheduleMode = ref<ScheduleMode>("cron");
const editingSchedule = ref<ScheduledMessageSummary | null>(null);
const form = reactive({
  templateId: null as number | null,
  providerId: null as number | null,
  model: "",
  name: "",
  prompt: "",
  cronExpr: "0 9 * * *",
  timezone: "Asia/Shanghai",
  scheduledAt: "",
});

const scheduleId = computed(() => routeParam("scheduleId"));
const isEditing = computed(() => Boolean(scheduleId.value));
const title = computed(() => (isEditing.value ? "编辑定时任务" : "创建定时任务"));
const submitLabel = computed(() => {
  if (submitting.value) return isEditing.value ? "保存中" : "创建中";
  return isEditing.value ? "保存任务" : "创建任务";
});

const selectedTemplate = computed(() =>
  templates.value.find((template) => template.id === Number(form.templateId)) ?? null,
);

const selectedProvider = computed(() =>
  providers.value.find((provider) => provider.id === Number(form.providerId)) ?? null,
);

const modelOptions = computed(() => selectedProvider.value?.models ?? []);

const canSubmit = computed(() => {
  const hasTarget = form.templateId;
  const hasPrompt = form.prompt.trim();
  const hasSchedule = scheduleMode.value === "cron" ? form.cronExpr.trim() : form.scheduledAt.trim();
  return Boolean(hasTarget && hasPrompt && hasSchedule && !submitting.value);
});

function routeParam(name: string): string {
  const value = route.params[name];
  if (Array.isArray(value)) return value[0] ?? "";
  return value ? String(value) : "";
}

function applyTemplateDefaults() {
  const template = selectedTemplate.value;
  if (!template) return;
  form.providerId = template.provider_id ?? providers.value.find((provider) => provider.is_default)?.id ?? providers.value[0]?.id ?? null;
  form.model = template.model_id ?? selectedProvider.value?.default_model ?? selectedProvider.value?.models[0] ?? "";
}

function applySchedule(schedule: ScheduledMessageSummary) {
  editingSchedule.value = schedule;
  form.templateId = schedule.template_id;
  form.providerId = schedule.provider_id;
  form.model = schedule.model ?? "";
  form.name = schedule.name;
  form.prompt = schedule.prompt;
  form.timezone = schedule.timezone ?? "Asia/Shanghai";
  if (schedule.cron_expr) {
    scheduleMode.value = "cron";
    form.cronExpr = schedule.cron_expr;
    form.scheduledAt = "";
  } else {
    scheduleMode.value = "once";
    form.cronExpr = "0 9 * * *";
    form.scheduledAt = schedule.scheduled_at ?? "";
  }
}

async function loadChatOptions() {
  const api = getApiClient();
  if (!api.getChatOptions) {
    error.value = "当前 API 客户端不支持加载 Agent 配置。";
    return;
  }

  const options = await api.getChatOptions();
  providers.value = options.providers;
  templates.value = options.templates;
  if (!isEditing.value && !form.templateId && templates.value[0]) {
    form.templateId = templates.value[0].id;
    applyTemplateDefaults();
  }
}

async function loadScheduleForEdit() {
  if (!isEditing.value) return;
  const api = getApiClient();
  if (!api.listScheduledMessages) {
    error.value = "当前 API 客户端不支持 Scheduler。";
    return;
  }
  const schedules = await api.listScheduledMessages();
  const schedule = schedules.find((item) => item.id === scheduleId.value);
  if (!schedule) {
    error.value = "未找到这个定时任务。";
    return;
  }
  applySchedule(schedule);
}

function buildRequest(): CreateScheduledMessageRequest {
  const input: CreateScheduledMessageRequest = {
    template_id: Number(form.templateId),
    provider_id: form.providerId === null ? null : Number(form.providerId),
    model: form.model.trim() || null,
    name: form.name.trim() || "Scheduled message",
    prompt: form.prompt.trim(),
  };
  if (scheduleMode.value === "cron") {
    input.cron_expr = form.cronExpr.trim();
    input.timezone = form.timezone.trim() || null;
  } else {
    input.scheduled_at = form.scheduledAt.trim();
  }
  return input;
}

async function submitSchedule() {
  const api = getApiClient();
  if (!canSubmit.value) {
    error.value = "请选择 Agent，并填写提示词和调度配置。";
    return;
  }
  if (!isEditing.value && !api.createScheduledMessage) {
    error.value = "当前 API 客户端不支持创建定时任务。";
    return;
  }
  if (isEditing.value && !api.updateScheduledMessage) {
    error.value = "当前 API 客户端不支持编辑定时任务。";
    return;
  }

  submitting.value = true;
  error.value = "";
  actionMessage.value = "";
  try {
    if (isEditing.value) {
      editingSchedule.value = await api.updateScheduledMessage!(scheduleId.value, buildRequest());
      actionMessage.value = "定时任务已保存。";
    } else {
      editingSchedule.value = await api.createScheduledMessage!(buildRequest());
      actionMessage.value = "定时任务已创建。";
    }
    await router.push("/scheduler");
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : "保存定时任务失败。";
  } finally {
    submitting.value = false;
  }
}

watch(
  () => form.templateId,
  () => {
    if (!loading.value) applyTemplateDefaults();
  },
);

watch(
  () => form.providerId,
  () => {
    const provider = selectedProvider.value;
    if (!provider) {
      form.model = "";
      return;
    }
    if (!provider.models.includes(form.model)) {
      form.model = provider.default_model || provider.models[0] || "";
    }
  },
);

onMounted(() => {
  void (async () => {
    try {
      await loadChatOptions();
      await loadScheduleForEdit();
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : "加载定时任务表单失败。";
    } finally {
      loading.value = false;
    }
  })();
});
</script>

<template>
  <section class="page-section">
    <div class="page-header">
      <div>
        <h3 class="page-title">{{ title }}</h3>
        <p>配置任务触发时使用的 Agent、模型与提示词。</p>
      </div>
      <a class="back-link" href="/scheduler">返回定时任务</a>
    </div>

    <p v-if="error" class="error-message">
      {{ error }}
    </p>
    <p v-if="actionMessage" class="success-message">
      {{ actionMessage }}
    </p>

    <div v-if="loading" class="loading-state">
      加载中...
    </div>

    <section v-else class="scheduler-panel">
      <div class="form-grid">
        <label>
          <span>Agent</span>
          <TinySelect v-model="form.templateId" data-testid="schedule-template-id">
            <TinyOption v-for="template in templates" :key="template.id" :label="template.display_name" :value="template.id" />
          </TinySelect>
        </label>
        <label>
          <span>Provider</span>
          <TinySelect v-model="form.providerId" data-testid="schedule-provider-id">
            <TinyOption v-for="provider in providers" :key="provider.id" :label="provider.display_name" :value="provider.id" />
          </TinySelect>
        </label>
        <label>
          <span>Model</span>
          <TinySelect v-model="form.model" data-testid="schedule-model">
            <TinyOption v-for="model in modelOptions" :key="model" :label="model" :value="model" />
          </TinySelect>
        </label>
        <label>
          <span>任务名称</span>
          <TinyInput v-model="form.name" data-testid="schedule-name" placeholder="每日检查" />
        </label>
        <label>
          <span>调度类型</span>
          <TinySelect v-model="scheduleMode" data-testid="schedule-mode">
            <TinyOption label="Cron" value="cron" />
            <TinyOption label="一次性" value="once" />
          </TinySelect>
        </label>
        <label class="form-wide">
          <span>提示词</span>
          <TinyInput v-model="form.prompt" data-testid="schedule-prompt" type="textarea" placeholder="到点后发送给新对话的用户消息" />
        </label>
        <template v-if="scheduleMode === 'cron'">
          <label>
            <span>Cron 表达式</span>
            <TinyInput v-model="form.cronExpr" data-testid="schedule-cron-expr" placeholder="0 9 * * *" />
          </label>
          <label>
            <span>时区</span>
            <TinyInput v-model="form.timezone" data-testid="schedule-timezone" placeholder="Asia/Shanghai" />
          </label>
        </template>
        <label v-else class="form-wide">
          <span>一次性时间</span>
          <TinyInput v-model="form.scheduledAt" data-testid="schedule-scheduled-at" placeholder="2026-05-10T01:00:00Z" />
        </label>
      </div>
      <div class="form-actions">
        <TinyButton data-testid="submit-schedule" type="primary" :disabled="!canSubmit" @click="submitSchedule">
          {{ submitLabel }}
        </TinyButton>
      </div>
    </section>
  </section>
</template>

<style scoped>
.page-section {
  display: grid;
  gap: var(--space-5);
}

.page-header,
.form-actions {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-3);
}

.page-title {
  margin: 0;
  color: var(--text-primary);
  font-size: var(--text-base);
  font-weight: 590;
}

.page-header p {
  margin: var(--space-2) 0 0;
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.back-link {
  flex: 0 0 auto;
  color: var(--color-primary);
  font-size: var(--text-sm);
  text-decoration: none;
}

.back-link:hover {
  text-decoration: underline;
}

.scheduler-panel,
.loading-state {
  background: var(--surface-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xs);
}

.scheduler-panel {
  display: grid;
  gap: var(--space-4);
  padding: var(--space-5);
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.form-grid label {
  display: grid;
  gap: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: 590;
}

.form-wide {
  grid-column: 1 / -1;
}

.form-actions {
  justify-content: flex-end;
}

.loading-state {
  padding: var(--space-10) var(--space-4);
  color: var(--text-muted);
  text-align: center;
}

.error-message,
.success-message {
  margin: 0;
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
}

.error-message {
  background: var(--status-danger-bg);
  color: var(--status-danger);
}

.success-message {
  background: var(--status-success-bg);
  color: var(--status-success);
}

@media (max-width: 960px) {
  .page-header {
    display: grid;
  }

  .form-grid {
    grid-template-columns: 1fr;
  }

  .form-wide {
    grid-column: auto;
  }
}
</style>
