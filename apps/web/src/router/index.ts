import { createRouter, createWebHistory } from "vue-router";

import AdminLayout from "@/layouts/AdminLayout.vue";
import BootstrapPage from "@/features/bootstrap/BootstrapPage.vue";
import HealthPage from "@/features/health/HealthPage.vue";
import RuntimePage from "@/features/runtime/RuntimePage.vue";
import ProvidersPage from "@/features/providers/ProvidersPage.vue";
import ProviderEditPage from "@/features/providers/ProviderEditPage.vue";
import TemplatesPage from "@/features/templates/TemplatesPage.vue";
import TemplateEditPage from "@/features/templates/TemplateEditPage.vue";
import McpPage from "@/features/mcp/McpPage.vue";
import McpEditPage from "@/features/mcp/McpEditPage.vue";
import McpImportPage from "@/features/mcp/McpImportPage.vue";
import ToolsPage from "@/features/tools/ToolsPage.vue";

const ChatPage = () => import("@/features/chat/ChatPage.vue");

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
          meta: { breadcrumb: "概览" },
        },
        {
          path: "health",
          name: "health",
          component: HealthPage,
          meta: { breadcrumb: "健康检查" },
        },
        {
          path: "runtime",
          name: "runtime",
          component: RuntimePage,
          meta: { breadcrumb: "运行状态" },
        },
        {
          path: "providers",
          children: [
            {
              path: "",
              name: "providers",
              component: ProvidersPage,
              meta: { breadcrumb: "模型提供方" },
            },
            {
              path: "new",
              name: "provider-new",
              component: ProviderEditPage,
              meta: { breadcrumb: "新增" },
            },
            {
              path: ":providerId/edit",
              name: "provider-edit",
              component: ProviderEditPage,
              meta: { breadcrumb: "编辑" },
            },
          ],
        },
        {
          path: "templates",
          children: [
            {
              path: "",
              name: "templates",
              component: TemplatesPage,
              meta: { breadcrumb: "智能体模板" },
            },
            {
              path: "new",
              name: "template-new",
              component: TemplateEditPage,
              meta: { breadcrumb: "新增" },
            },
            {
              path: ":templateId/edit",
              name: "template-edit",
              component: TemplateEditPage,
              meta: { breadcrumb: "编辑" },
            },
          ],
        },
        {
          path: "mcp",
          children: [
            {
              path: "",
              name: "mcp",
              component: McpPage,
              meta: { breadcrumb: "MCP 服务" },
            },
            {
              path: "new",
              name: "mcp-new",
              component: McpEditPage,
              meta: { breadcrumb: "新增" },
            },
            {
              path: "import",
              name: "mcp-import",
              component: McpImportPage,
              meta: { breadcrumb: "JSON 导入" },
            },
            {
              path: ":serverId/edit",
              name: "mcp-edit",
              component: McpEditPage,
              meta: { breadcrumb: "编辑" },
            },
          ],
        },
        {
          path: "tools",
          name: "tools",
          component: ToolsPage,
          meta: { breadcrumb: "工具注册表" },
        },
        {
          path: "chat",
          name: "chat",
          component: ChatPage,
          meta: { breadcrumb: "对话" },
        },
      ],
    },
  ],
});

export default router;
