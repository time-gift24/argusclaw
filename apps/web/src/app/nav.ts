export interface AdminNavItem {
  key: string;
  label: string;
  description: string;
  to: string;
}

export const adminNavItems: AdminNavItem[] = [
  {
    key: "bootstrap",
    label: "概览",
    description: "实例标识、默认配置与启动摘要",
    to: "/",
  },
  {
    key: "health",
    label: "健康检查",
    description: "服务连通性与当前实例状态",
    to: "/health",
  },
  {
    key: "runtime",
    label: "运行状态",
    description: "线程池与后台 job runtime 的负载快照",
    to: "/runtime",
  },
  {
    key: "providers",
    label: "模型提供方",
    description: "模型接入凭证与默认项管理",
    to: "/providers",
  },
  {
    key: "templates",
    label: "智能体模板",
    description: "内置与自定义模板配置",
    to: "/templates",
  },
  {
    key: "mcp",
    label: "MCP 服务",
    description: "外部工具连接与传输状态",
    to: "/mcp",
  },
  {
    key: "settings",
    label: "系统设置",
    description: "实例级管理偏好与默认行为",
    to: "/settings",
  },
];
