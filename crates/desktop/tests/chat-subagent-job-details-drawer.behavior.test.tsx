import test from "node:test";
import assert from "node:assert/strict";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import type { JobDetailPayload } from "../lib/types/chat";

const localStorageStub = {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
};

Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: {
    __TAURI_INTERNALS__: {
      invoke: async () => null,
      transformCallback: () => 0,
    },
    localStorage: localStorageStub,
    matchMedia: () => ({
      matches: false,
      addEventListener: () => {},
      removeEventListener: () => {},
    }),
  },
});

Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: localStorageStub,
});

Object.defineProperty(globalThis, "document", {
  configurable: true,
  value: {
    documentElement: {
      classList: {
        add: () => {},
        remove: () => {},
        contains: () => false,
        toggle: () => {},
      },
    },
  },
});

const { SubagentJobDetailsPanel } = await import(
  "../components/assistant-ui/subagent-job-details-drawer"
);

function makeDetail(
  overrides: Partial<JobDetailPayload> & Pick<JobDetailPayload, "job_id">,
): JobDetailPayload {
  return {
    job_id: overrides.job_id,
    agent_id: overrides.agent_id ?? 101,
    agent_display_name: overrides.agent_display_name ?? "Worker",
    agent_description: overrides.agent_description ?? "Background worker",
    prompt: overrides.prompt ?? "Investigate the issue",
    status: overrides.status ?? "running",
    summary_text: overrides.summary_text ?? "Summary",
    result_text: overrides.result_text ?? null,
    started_at: overrides.started_at ?? "2026-04-08T00:00:00.000Z",
    finished_at: overrides.finished_at ?? null,
    input_tokens: overrides.input_tokens ?? null,
    output_tokens: overrides.output_tokens ?? null,
    source_message_id: overrides.source_message_id ?? null,
    thread_id: overrides.thread_id ?? "thread-parent",
    timeline: overrides.timeline ?? [],
  };
}

test("drawer panel renders selected detail output and metadata", () => {
  const html = renderToStaticMarkup(
    React.createElement(SubagentJobDetailsPanel, {
      detail: makeDetail({
        job_id: "job-1",
        result_text: "完整输出",
        summary_text: "摘要",
        timeline: [
          {
            kind: "dispatched",
            at: "2026-04-08T00:00:00.000Z",
            label: "已派发",
            status: "running",
          },
        ],
      }),
    }),
  );

  assert.match(html, /完整输出/);
  assert.match(html, /任务信息/);
  assert.match(html, /执行过程/);
  assert.match(html, /Worker/);
});

test("drawer panel falls back to the summary when no full result exists", () => {
  const html = renderToStaticMarkup(
    React.createElement(SubagentJobDetailsPanel, {
      detail: makeDetail({
        job_id: "job-2",
        result_text: null,
        summary_text: "任务摘要",
      }),
    }),
  );

  assert.match(html, /结果摘要/);
  assert.match(html, /任务摘要/);
  assert.match(html, /Job job-2/);
});
