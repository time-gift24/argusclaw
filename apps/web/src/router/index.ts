import { createRouter, createWebHistory } from "vue-router";

import AdminLayout from "@/layouts/AdminLayout.vue";
import BootstrapPage from "@/features/bootstrap/BootstrapPage.vue";
import HealthPage from "@/features/health/HealthPage.vue";
import ProvidersPage from "@/features/providers/ProvidersPage.vue";
import TemplatesPage from "@/features/templates/TemplatesPage.vue";
import McpPage from "@/features/mcp/McpPage.vue";
import SettingsPage from "@/features/settings/SettingsPage.vue";

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: "/",
      component: AdminLayout,
      children: [
        {
          path: "",
          name: "bootstrap",
          component: BootstrapPage,
        },
        {
          path: "health",
          name: "health",
          component: HealthPage,
        },
        {
          path: "providers",
          name: "providers",
          component: ProvidersPage,
        },
        {
          path: "templates",
          name: "templates",
          component: TemplatesPage,
        },
        {
          path: "mcp",
          name: "mcp",
          component: McpPage,
        },
        {
          path: "settings",
          name: "settings",
          component: SettingsPage,
        },
      ],
    },
  ],
});

export default router;
