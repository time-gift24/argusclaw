import * as React from "react";
import {
  createBrowserRouter,
  Outlet,
  type RouteObject,
} from "react-router-dom";

import RootLayout from "@/app/layout";
import SettingsLayout from "@/app/settings/layout";

const ChatPage = React.lazy(() => import("@/app/page"));
const SettingsPage = React.lazy(() => import("@/app/settings/page"));
const ProvidersPage = React.lazy(() => import("@/app/settings/providers/page"));
const NewProviderPage = React.lazy(() => import("@/app/settings/providers/new/page"));
const EditProviderPage = React.lazy(() => import("@/app/settings/providers/edit/page"));
const AgentsPage = React.lazy(() => import("@/app/settings/agents/page"));
const NewAgentPage = React.lazy(() => import("@/app/settings/agents/new/page"));
const EditAgentPage = React.lazy(() => import("@/app/settings/agents/edit/page"));
const KnowledgePage = React.lazy(() => import("@/app/settings/knowledge/page"));
const ToolsPage = React.lazy(() => import("@/app/settings/tools/page"));
const McpPage = React.lazy(() => import("@/app/settings/mcp/page"));
const NewMcpPage = React.lazy(() => import("@/app/settings/mcp/new/page"));
const EditMcpPage = React.lazy(() => import("@/app/settings/mcp/edit/page"));
const McpPage = React.lazy(() => import("@/app/settings/mcp/page"));
const NewMcpPage = React.lazy(() => import("@/app/settings/mcp/new/page"));
const EditMcpPage = React.lazy(() => import("@/app/settings/mcp/edit/page"));

function withSuspense(element: React.ReactNode) {
  return (
    <React.Suspense
      fallback={
        <div className="flex items-center justify-center h-64">
          <div className="text-muted-foreground">加载中...</div>
        </div>
      }
    >
      {element}
    </React.Suspense>
  );
}

export const desktopRoutes: RouteObject[] = [
  {
    path: "/",
    element: (
      <RootLayout>
        <Outlet />
      </RootLayout>
    ),
    children: [
      {
        index: true,
        element: withSuspense(<ChatPage />),
      },
      {
        path: "settings",
        element: (
          <SettingsLayout>
            <Outlet />
          </SettingsLayout>
        ),
        children: [
          {
            index: true,
            element: withSuspense(<SettingsPage />),
          },
          {
            path: "providers",
            element: withSuspense(<ProvidersPage />),
          },
          {
            path: "providers/new",
            element: withSuspense(<NewProviderPage />),
          },
          {
            path: "providers/edit",
            element: withSuspense(<EditProviderPage />),
          },
          {
            path: "agents",
            element: withSuspense(<AgentsPage />),
          },
          {
            path: "agents/new",
            element: withSuspense(<NewAgentPage />),
          },
          {
            path: "agents/edit",
            element: withSuspense(<EditAgentPage />),
          },
          {
            path: "knowledge",
            element: withSuspense(<KnowledgePage />),
          },
          {
            path: "tools",
            element: withSuspense(<ToolsPage />),
          },
          {
            path: "mcp",
            element: withSuspense(<McpPage />),
          },
          {
            path: "mcp/new",
            element: withSuspense(<NewMcpPage />),
          },
          {
            path: "mcp/edit",
            element: withSuspense(<EditMcpPage />),
          },
        ],
      },
    ],
  },
];

export function createDesktopRouter() {
  return createBrowserRouter(desktopRoutes);
}
