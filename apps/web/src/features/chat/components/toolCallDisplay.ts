import type { ToolActivityStatus } from "../composables/useChatThreadStream";

export function toolIcon(kind: string) {
  if (kind === "shell") return "⌘";
  if (kind === "mcp") return "M";
  if (kind === "search") return "⌕";
  if (kind === "http") return "⇄";
  if (kind === "file") return "F";
  if (kind === "job") return "JOB";
  return "T";
}

export function toolKindLabel(kind: string) {
  if (kind === "shell") return "命令执行";
  if (kind === "mcp") return "MCP 服务";
  if (kind === "search") return "检索";
  if (kind === "http") return "网络请求";
  if (kind === "file") return "文件操作";
  if (kind === "job") return "后台 Job";
  return "通用工具";
}

export function statusLabel(status: ToolActivityStatus) {
  if (status === "success") return "完成";
  if (status === "error") return "失败";
  return "运行中";
}

export function previewText(value: string, emptyText: string) {
  return value.trim() || emptyText;
}

export function toolKindFromName(name: string) {
  if (name === "shell" || name.startsWith("shell.") || name === "exec") return "shell";
  if (name.startsWith("mcp.")) return "mcp";
  if (name.includes("search")) return "search";
  if (name.includes("http") || name.includes("fetch")) return "http";
  if (name.includes("file") || name.includes("fs")) return "file";
  return "tool";
}
