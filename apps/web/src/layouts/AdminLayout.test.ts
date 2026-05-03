import { mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { reactive } from "vue";

const mockRoute = reactive<{ path: string; meta: Record<string, unknown> }>({
  path: "/chat",
  meta: {},
});
const replace = vi.fn();
const getBootstrap = vi.fn();

vi.mock("vue-router", () => ({
  useRoute: () => mockRoute,
  useRouter: () => ({ replace }),
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

vi.mock("@/lib/api", () => ({
  getApiClient: () => ({ getBootstrap }),
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
    replace.mockReset();
    getBootstrap.mockResolvedValue({
      instance_name: "ArgusWing",
      provider_count: 0,
      template_count: 0,
      mcp_server_count: 0,
      default_provider_id: 1,
      default_template_id: null,
      mcp_ready_count: 0,
      current_user: {
        id: "00000000-0000-0000-0000-000000000000",
        external_id: "admin-user",
        display_name: null,
        is_admin: true,
      },
    });
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

  it("shows only the chat tab for ordinary users and redirects management routes", async () => {
    getBootstrap.mockResolvedValueOnce({
      instance_name: "ArgusWing",
      provider_count: 0,
      template_count: 0,
      mcp_server_count: 0,
      default_provider_id: 1,
      default_template_id: null,
      mcp_ready_count: 0,
      current_user: {
        id: "11111111-1111-1111-1111-111111111111",
        external_id: "ordinary-user",
        display_name: null,
        is_admin: false,
      },
    });
    mockRoute.path = "/providers";
    mockRoute.meta = {};

    const wrapper = mount(AdminLayout);
    await flushPromises();

    expect(wrapper.text()).toContain("对话");
    expect(wrapper.text()).not.toContain("模型提供方");
    expect(wrapper.text()).not.toContain("运行状态");
    expect(replace).toHaveBeenCalledWith("/chat");
  });
});
