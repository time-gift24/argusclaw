import { mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { reactive } from "vue";

const mockRoute = reactive<{ path: string; meta: Record<string, unknown> }>({
  path: "/chat",
  meta: {},
});

vi.mock("vue-router", () => ({
  useRoute: () => mockRoute,
  RouterLink: {
    name: "RouterLink",
    props: ["to"],
    template: "<a :data-to=\"to\"><slot /></a>",
  },
  RouterView: {
    name: "RouterView",
    template: "<div class='router-view-stub' />",
  },
}));

vi.mock("@/components/AppBreadcrumb.vue", () => ({
  default: {
    name: "AppBreadcrumbStub",
    template: "<nav class='app-breadcrumb-stub'>crumb</nav>",
  },
}));

import AdminLayout from "./AdminLayout.vue";

describe("AdminLayout", () => {
  beforeEach(() => {
    mockRoute.path = "/chat";
    mockRoute.meta = {
      hideRouteHeader: true,
      immersive: true,
    };
  });

  afterEach(() => {
    localStorage.clear();
    document.documentElement.classList.remove("theme-light", "theme-dark");
  });

  it("hides the shared route header and enables immersive shell classes for chat", () => {
    const wrapper = mount(AdminLayout);

    expect(wrapper.find(".route-header").exists()).toBe(false);
    expect(wrapper.find(".route-shell--immersive").exists()).toBe(true);
  });

  it("keeps the shared route header for standard admin pages", () => {
    mockRoute.path = "/providers";
    mockRoute.meta = {};

    const wrapper = mount(AdminLayout);

    expect(wrapper.find(".route-header").exists()).toBe(true);
    expect(wrapper.find(".route-shell--immersive").exists()).toBe(false);
    expect(wrapper.text()).toContain("模型提供方");
  });
});
