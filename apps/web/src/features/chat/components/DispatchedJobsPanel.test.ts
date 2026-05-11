import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import type { ChatThreadJobSummary } from "@/lib/api";
import DispatchedJobsPanel from "./DispatchedJobsPanel.vue";

function job(overrides: Partial<ChatThreadJobSummary> = {}): ChatThreadJobSummary {
  return {
    job_id: "job-1",
    title: "分析日志",
    subagent_name: "researcher",
    status: "running",
    created_at: "2026-05-11T09:00:00Z",
    updated_at: "2026-05-11T09:01:00Z",
    result_preview: "正在分析",
    bound_thread_id: "thread-job-1",
    ...overrides,
  };
}

describe("DispatchedJobsPanel", () => {
  it("shows the empty state when there are no dispatched subagents", () => {
    const wrapper = mount(DispatchedJobsPanel, {
      props: { jobs: [], loading: false, error: "" },
    });

    expect(wrapper.text()).toContain("已派发 subagent");
    expect(wrapper.text()).toContain("暂无派发的 subagent");
  });

  it("emits openJob when clicking a dispatched job row", async () => {
    const wrapper = mount(DispatchedJobsPanel, {
      props: { jobs: [job()], loading: false, error: "" },
    });

    await wrapper.get("[data-testid='dispatched-job-job-1']").trigger("click");

    expect(wrapper.emitted("openJob")).toEqual([["job-1"]]);
  });

  it("shows loading and error states", () => {
    const loadingWrapper = mount(DispatchedJobsPanel, {
      props: { jobs: [], loading: true, error: "" },
    });
    expect(loadingWrapper.text()).toContain("正在加载派发记录...");

    const errorWrapper = mount(DispatchedJobsPanel, {
      props: { jobs: [], loading: false, error: "Request failed" },
    });
    expect(errorWrapper.text()).toContain("派发记录加载失败，可刷新重试");
  });

  it("emits refresh when clicking the refresh button", async () => {
    const wrapper = mount(DispatchedJobsPanel, {
      props: { jobs: [job()], loading: false, error: "" },
    });

    await wrapper.get("button[title='刷新']").trigger("click");

    expect(wrapper.emitted("refresh")).toHaveLength(1);
  });
});
