"use client"

import * as React from "react"
import { useRouter } from "next/navigation"
import {
  ArrowLeft,
  CheckCircle2,
  Globe,
  Loader2,
  Network,
  Plus,
  Save,
  Server,
  TerminalSquare,
  Trash2,
} from "lucide-react"

import {
  mcp,
  type McpConnectionTestResult,
  type McpDiscoveredToolRecord,
  type McpServerRecord,
  type McpTransportConfig,
} from "@/lib/tauri"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { useToast } from "@/components/ui/toast"
import { McpTestDialog } from "@/components/settings/mcp-test-dialog"
import { cn } from "@/lib/utils"

interface McpEditorProps {
  serverId?: number
}

interface KeyValueRow {
  id: string
  key: string
  value: string
}

let keyValueRowCounter = 0

function createDefaultFormData(): McpServerRecord {
  return {
    id: null,
    display_name: "",
    enabled: true,
    transport: {
      kind: "stdio",
      command: "",
      args: ["--stdio"],
      env: {},
    },
    timeout_ms: 30000,
    status: "failed",
    last_checked_at: null,
    last_success_at: null,
    last_error: null,
    discovered_tool_count: 0,
  }
}

function formatList(values: string[]): string {
  return values.join("\n")
}

function parseList(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
}

function createKeyValueRow(key = "", value = ""): KeyValueRow {
  keyValueRowCounter += 1
  return {
    id: `key-value-row-${keyValueRowCounter}`,
    key,
    value,
  }
}

function formatKeyValueLines(values: Record<string, string>): string {
  return Object.entries(values)
    .map(([key, value]) => `${key}=${value}`)
    .join("\n")
}

function recordToRows(values: Record<string, string>): KeyValueRow[] {
  const rows = Object.entries(values).map(([key, value]) => createKeyValueRow(key, value))
  return rows.length > 0 ? rows : [createKeyValueRow()]
}

function rowsToRecord(rows: KeyValueRow[]): Record<string, string> {
  return rows.reduce<Record<string, string>>((acc, row) => {
    const key = row.key.trim()
    if (!key) return acc
    acc[key] = row.value.trim()
    return acc
  }, {})
}

function getStatusTone(status: McpServerRecord["status"]) {
  switch (status) {
    case "ready":
      return "bg-emerald-500/10 text-emerald-700"
    case "disabled":
      return "bg-slate-500/10 text-slate-600"
    case "failed":
      return "bg-rose-500/10 text-rose-700"
    default:
      return "bg-amber-500/10 text-amber-700"
  }
}

export function McpEditor({ serverId }: McpEditorProps) {
  const router = useRouter()
  const { addToast } = useToast()
  const isEditing = serverId !== undefined

  const [loading, setLoading] = React.useState(isEditing)
  const [saving, setSaving] = React.useState(false)
  const [testing, setTesting] = React.useState(false)
  const [formData, setFormData] = React.useState<McpServerRecord>(createDefaultFormData)
  const [argsText, setArgsText] = React.useState("--stdio")
  const [envText, setEnvText] = React.useState("")
  const [headerRows, setHeaderRows] = React.useState<KeyValueRow[]>([createKeyValueRow()])
  const [testResult, setTestResult] = React.useState<McpConnectionTestResult | null>(null)
  const [discoveredTools, setDiscoveredTools] = React.useState<McpDiscoveredToolRecord[]>([])
  const [testDialogOpen, setTestDialogOpen] = React.useState(false)

  const applyRecord = React.useCallback((record: McpServerRecord) => {
    setFormData(record)
    if (record.transport.kind === "stdio") {
      setArgsText(formatList(record.transport.args))
      setEnvText(formatKeyValueLines(record.transport.env))
      setHeaderRows([createKeyValueRow()])
    } else {
      setArgsText("")
      setEnvText("")
      setHeaderRows(recordToRows(record.transport.headers))
    }
  }, [])

  React.useEffect(() => {
    const loadData = async () => {
      if (!serverId) {
        setLoading(false)
        return
      }

      try {
        const [record, toolSnapshot] = await Promise.all([
          mcp.getServer(serverId),
          mcp.listServerTools(serverId).catch(() => []),
        ])
        if (record) {
          applyRecord(record)
          setDiscoveredTools(toolSnapshot)
        }
      } catch (error) {
        console.error("Failed to load MCP server:", error)
      } finally {
        setLoading(false)
      }
    }

    void loadData()
  }, [applyRecord, serverId])

  const canSave = React.useMemo(() => {
    if (!formData.display_name.trim()) return false
    if (formData.transport.kind === "stdio") {
      return formData.transport.command.trim().length > 0
    }
    return formData.transport.url.trim().length > 0
  }, [formData])

  const buildRecord = React.useCallback((): McpServerRecord => {
    const baseRecord: McpServerRecord = {
      ...formData,
      timeout_ms: Number.isFinite(formData.timeout_ms) ? formData.timeout_ms : 30000,
    }

    let transport: McpTransportConfig
    if (formData.transport.kind === "stdio") {
      transport = {
        kind: "stdio",
        command: formData.transport.command,
        args: parseList(argsText),
        env: parseKeyValueLines(envText),
      }
    } else {
      transport = {
        kind: formData.transport.kind,
        url: formData.transport.url,
        headers: rowsToRecord(headerRows),
      }
    }

    return {
      ...baseRecord,
      transport,
    }
  }, [argsText, envText, formData, headerRows])

  const handleTransportKindChange = (kind: McpTransportConfig["kind"]) => {
    if (kind === formData.transport.kind) return

    if (kind === "stdio") {
      setFormData((prev) => ({
        ...prev,
        transport: {
          kind: "stdio",
          command: "",
          args: [],
          env: {},
        },
      }))
      setArgsText("--stdio")
      setEnvText("")
      setHeaderRows([createKeyValueRow()])
      return
    }

    setFormData((prev) => ({
      ...prev,
      transport: {
        kind,
        url: "",
        headers: {},
      },
    }))
    setArgsText("")
    setEnvText("")
    setHeaderRows([createKeyValueRow()])
  }

  const updateHeaderRow = React.useCallback((rowId: string, field: "key" | "value", value: string) => {
    setHeaderRows((prev) =>
      prev.map((row) => (row.id === rowId ? { ...row, [field]: value } : row)),
    )
  }, [])

  const addHeaderRow = React.useCallback(() => {
    setHeaderRows((prev) => [...prev, createKeyValueRow()])
  }, [])

  const removeHeaderRow = React.useCallback((rowId: string) => {
    setHeaderRows((prev) => {
      if (prev.length === 1) {
        return [createKeyValueRow()]
      }

      const next = prev.filter((row) => row.id !== rowId)
      return next.length > 0 ? next : [createKeyValueRow()]
    })
  }, [])

  const handleSave = async () => {
    if (!canSave) return
    setSaving(true)
    try {
      const id = await mcp.upsertServer(buildRecord())
      addToast("success", isEditing ? "MCP 配置已更新" : "MCP 配置已创建")
      router.push(`/settings/mcp/edit?id=${id}`)
    } catch (error) {
      console.error("Failed to save MCP server:", error)
      addToast("error", "保存 MCP 配置失败")
    } finally {
      setSaving(false)
    }
  }

  const handleTestConnection = async () => {
    const record = buildRecord()
    setTesting(true)
    try {
      const result = record.id ? await mcp.testConnection(record.id) : await mcp.testInput(record)
      setTestResult(result)
      setDiscoveredTools(result.discovered_tools)
      setTestDialogOpen(true)

      if (record.id) {
        const refreshed = await mcp.getServer(record.id)
        if (refreshed) {
          applyRecord(refreshed)
        }
      }
    } catch (error) {
      const fallbackResult: McpConnectionTestResult = {
        status: "failed",
        checked_at: new Date().toISOString(),
        latency_ms: 0,
        discovered_tools: [],
        message: error instanceof Error ? error.message : String(error),
      }
      setTestResult(fallbackResult)
      setTestDialogOpen(true)
      console.error("Failed to test MCP server:", error)
    } finally {
      setTesting(false)
    }
  }

  if (loading) {
    return (
      <div className="flex flex-col items-center justify-center h-64 gap-3">
        <div className="h-8 w-8 border-4 border-primary border-t-transparent rounded-full animate-spin" />
        <div className="text-muted-foreground text-sm">正在加载 MCP 配置...</div>
      </div>
    )
  }

  return (
    <div className="w-full h-full flex flex-col min-h-0 animate-in fade-in duration-500 overflow-hidden">
      <div className="flex items-center justify-between border-b pb-6 shrink-0 px-1">
        <div className="flex items-center gap-4">
          <Button
            variant="ghost"
            size="icon"
            className="h-9 w-9 rounded-full hover:bg-muted"
            onClick={() => router.push("/settings/mcp")}
          >
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <div className="space-y-0.5">
            <h1 className="text-lg font-bold tracking-tight">{isEditing ? "编辑 MCP Server" : "新建 MCP Server"}</h1>
            <p className="text-[11px] text-muted-foreground uppercase tracking-wider font-semibold opacity-70">
              MCP Configuration / {isEditing ? formData.display_name : "New Server"}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="sm" onClick={() => router.push("/settings/mcp")} className="h-9 text-sm text-muted-foreground hover:text-foreground">
            取消
          </Button>
          <Button size="sm" variant="outline" onClick={handleTestConnection} disabled={testing || !canSave} className="h-9 px-4">
            {testing ? <Loader2 className="h-4 w-4 mr-2 animate-spin" /> : <Network className="h-4 w-4 mr-2" />}
            测试连接
          </Button>
          <Button size="sm" onClick={handleSave} disabled={saving || !canSave} className="h-9 px-6 text-sm font-bold shadow-lg shadow-primary/20">
            <Save className="h-4 w-4 mr-2" />
            {saving ? "正在保存..." : "保存配置"}
          </Button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto custom-scrollbar px-1 py-8">
        <div className="space-y-10 pb-20">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-8 items-stretch">
            <div className="flex flex-col h-full space-y-4">
              <div className="flex items-center gap-2 text-[11px] font-bold text-primary uppercase tracking-widest px-1">
                <div className="bg-primary/10 p-1.5 rounded-lg text-primary">
                  <Server className="h-3.5 w-3.5" />
                </div>
                Server Details
              </div>
              <div className="flex-1 flex flex-col justify-between gap-6 bg-muted/20 p-6 rounded-[24px] border border-muted/60 shadow-sm">
                <div className="space-y-2">
                  <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">显示名称</Label>
                  <Input
                    value={formData.display_name}
                    onChange={(event) => setFormData((prev) => ({ ...prev, display_name: event.target.value }))}
                    placeholder="例如: Slack MCP"
                    className="h-10 bg-background border-muted/60 text-sm"
                  />
                </div>

                <div className="space-y-2">
                  <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">Transport</Label>
                  <select
                    value={formData.transport.kind}
                    onChange={(event) => handleTransportKindChange(event.target.value as McpTransportConfig["kind"])}
                    className="flex h-10 w-full rounded-md border border-muted/60 bg-background px-3 py-1.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-primary/20 appearance-none"
                  >
                    <option value="stdio">stdio</option>
                    <option value="http">http</option>
                    <option value="sse">sse</option>
                  </select>
                </div>

                <div className="space-y-2">
                  <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">请求超时 (ms)</Label>
                  <Input
                    type="number"
                    value={formData.timeout_ms}
                    onChange={(event) => setFormData((prev) => ({ ...prev, timeout_ms: parseInt(event.target.value, 10) || 30000 }))}
                    className="h-10 bg-background border-muted/60 text-sm font-mono"
                  />
                </div>

                <div className="flex items-center gap-3 bg-background/50 p-3 rounded-xl border border-muted/40 h-14 shadow-inner">
                  <Checkbox
                    id="mcp-enabled"
                    checked={formData.enabled}
                    onCheckedChange={(checked) => setFormData((prev) => ({ ...prev, enabled: !!checked }))}
                  />
                  <Label htmlFor="mcp-enabled" className="text-sm cursor-pointer font-bold">
                    启用该 MCP Server
                  </Label>
                </div>
              </div>
            </div>

            <div className="flex flex-col h-full space-y-4">
              <div className="flex items-center gap-2 text-[11px] font-bold text-primary uppercase tracking-widest px-1">
                <div className="bg-primary/10 p-1.5 rounded-lg text-primary">
                  {formData.transport.kind === "stdio" ? <TerminalSquare className="h-3.5 w-3.5" /> : <Globe className="h-3.5 w-3.5" />}
                </div>
                Transport Parameters
              </div>
              <div className="flex-1 flex flex-col justify-between gap-6 bg-muted/20 p-6 rounded-[24px] border border-muted/60 shadow-sm">
                {formData.transport.kind === "stdio" ? (
                  <>
                    <div className="space-y-2">
                      <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">Command</Label>
                      <Input
                        value={formData.transport.command}
                        onChange={(event) =>
                          setFormData((prev) => ({
                            ...prev,
                            transport: {
                              kind: "stdio",
                              command: event.target.value,
                              args: prev.transport.kind === "stdio" ? prev.transport.args : [],
                              env: prev.transport.kind === "stdio" ? prev.transport.env : {},
                            },
                          }))
                        }
                        placeholder="例如: npx @modelcontextprotocol/server-slack"
                        className="h-10 bg-background border-muted/60 text-sm font-mono"
                      />
                    </div>

                    <div className="space-y-2">
                      <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">Args (每行一个)</Label>
                      <Textarea
                        value={argsText}
                        onChange={(event) => setArgsText(event.target.value)}
                        placeholder="--stdio"
                        className="min-h-[110px] bg-background border-muted/60 text-sm font-mono"
                      />
                    </div>

                    <div className="space-y-2">
                      <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">Environment (KEY=VALUE)</Label>
                      <Textarea
                        value={envText}
                        onChange={(event) => setEnvText(event.target.value)}
                        placeholder="SLACK_TOKEN=xoxb-..."
                        className="min-h-[140px] bg-background border-muted/60 text-sm font-mono"
                      />
                    </div>
                  </>
                ) : (
                  <>
                    <div className="space-y-2">
                      <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">URL</Label>
                      <Input
                        value={formData.transport.url}
                        onChange={(event) =>
                          setFormData((prev) => ({
                            ...prev,
                            transport: {
                              kind: prev.transport.kind === "sse" ? "sse" : "http",
                              url: event.target.value,
                              headers: prev.transport.kind !== "stdio" ? prev.transport.headers : {},
                            },
                          }))
                        }
                        placeholder="https://example.com/mcp"
                        className="h-10 bg-background border-muted/60 text-sm font-mono"
                      />
                    </div>

                    <div className="space-y-2">
                      <div className="flex items-center justify-between gap-3">
                        <Label className="text-[11px] font-bold text-muted-foreground uppercase tracking-wider ml-1">Headers</Label>
                        <Button
                          type="button"
                          size="icon"
                          variant="outline"
                          className="h-8 w-8 shrink-0"
                          onClick={addHeaderRow}
                        >
                          <Plus className="h-4 w-4" />
                        </Button>
                      </div>

                      <div className="space-y-3">
                        {headerRows.map((row, index) => (
                          <div
                            key={row.id}
                            className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-2 items-center"
                          >
                            <Input
                              value={row.key}
                              onChange={(event) => updateHeaderRow(row.id, "key", event.target.value)}
                              placeholder={index === 0 ? "Header 名称" : "例如 Authorization"}
                              className="h-10 bg-background border-muted/60 text-sm font-mono"
                            />
                            <Input
                              value={row.value}
                              onChange={(event) => updateHeaderRow(row.id, "value", event.target.value)}
                              placeholder={index === 0 ? "Header 值" : "例如 Bearer ..."}
                              className="h-10 bg-background border-muted/60 text-sm font-mono"
                            />
                            <Button
                              type="button"
                              size="icon"
                              variant="ghost"
                              className="h-9 w-9 shrink-0 text-muted-foreground hover:text-destructive"
                              onClick={() => removeHeaderRow(row.id)}
                            >
                              <Trash2 className="h-4 w-4" />
                            </Button>
                          </div>
                        ))}
                      </div>
                    </div>
                  </>
                )}
              </div>
            </div>
          </div>

          <div className="space-y-4">
            <div className="flex items-center gap-2 text-sm font-bold text-foreground px-1">
              <div className="bg-primary/10 p-1.5 rounded-lg text-primary">
                <CheckCircle2 className="h-4 w-4" />
              </div>
              连接测试与工具发现
            </div>
            <div className="bg-muted/10 rounded-[32px] border border-muted/60 p-6 shadow-sm space-y-5">
              <div className="flex flex-wrap items-center gap-3">
                <Badge className={cn("text-[10px] uppercase", getStatusTone(formData.status))}>
                  {formData.status}
                </Badge>
                {formData.last_checked_at && (
                  <span className="text-[11px] text-muted-foreground">
                    最近检查: {formData.last_checked_at}
                  </span>
                )}
                {formData.last_success_at && (
                  <span className="text-[11px] text-muted-foreground">
                    最近成功: {formData.last_success_at}
                  </span>
                )}
              </div>

              {testResult ? (
                <div className={cn(
                  "rounded-2xl border px-4 py-3 space-y-2",
                  testResult.status === "ready"
                    ? "border-emerald-500/30 bg-emerald-500/5"
                    : "border-rose-500/30 bg-rose-500/5",
                )}>
                  <div className="flex items-center justify-between gap-3">
                    <p className="text-sm font-semibold">{testResult.message}</p>
                    <div className="flex items-center gap-3">
                      <span className="text-[11px] text-muted-foreground">{testResult.latency_ms} ms</span>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-7 px-2 text-[11px]"
                        onClick={() => setTestDialogOpen(true)}
                      >
                        查看详情
                      </Button>
                    </div>
                  </div>
                  <p className="text-[11px] text-muted-foreground">
                    checked_at: {testResult.checked_at}
                  </p>
                </div>
              ) : (
                <div className="rounded-2xl border border-dashed border-muted/60 px-4 py-3 text-sm text-muted-foreground">
                  点击“测试连接”后会显示连接结果，并拉取当前 server 暴露的 MCP tools。
                </div>
              )}

              <div className="space-y-3">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <p className="text-sm font-semibold">Discovered Tools</p>
                    <p className="text-[11px] text-muted-foreground">
                      当前缓存 {discoveredTools.length} 个 tools
                    </p>
                  </div>
                </div>

                {discoveredTools.length === 0 ? (
                  <div className="rounded-2xl border border-dashed border-muted/60 px-4 py-5 text-sm text-muted-foreground">
                    还没有发现到工具。保存后可继续测试，或检查 transport 配置是否正确。
                  </div>
                ) : (
                  <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                    {discoveredTools.map((tool) => (
                      <div
                        key={`${tool.server_id}-${tool.tool_name_original}`}
                        className="rounded-2xl border border-muted/60 bg-background px-4 py-3 space-y-1"
                      >
                        <p className="text-sm font-semibold truncate">{tool.tool_name_original}</p>
                        <p className="text-[11px] text-muted-foreground line-clamp-3">
                          {tool.description || "No description"}
                        </p>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>

      <McpTestDialog
        open={testDialogOpen}
        onOpenChange={setTestDialogOpen}
        result={testResult}
        discoveredTools={discoveredTools}
        testing={testing}
        serverName={formData.display_name}
        onRetest={() => void handleTestConnection()}
      />
    </div>
  )
}
