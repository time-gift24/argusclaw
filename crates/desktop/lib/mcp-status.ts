import type { McpServerStatus } from "./tauri";

export const STATUS_COLORS: Record<string, string> = {
  connected: "bg-green-100 text-green-800",
  connecting: "bg-yellow-100 text-yellow-800",
  disconnected: "bg-gray-100 text-gray-600",
  failed: "bg-red-100 text-red-800",
};

export const STATUS_LABELS: Record<string, string> = {
  connected: "已连接",
  connecting: "连接中",
  disconnected: "未连接",
  failed: "连接失败",
};

export function getStatusColor(status: McpServerStatus): string {
  return STATUS_COLORS[status.status] ?? "bg-gray-100 text-gray-600";
}

export function getStatusLabel(status: McpServerStatus): string {
  return STATUS_LABELS[status.status] ?? "未知";
}
