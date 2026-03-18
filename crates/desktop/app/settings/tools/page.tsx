"use client"

import * as React from "react"
import { tools, type ToolInfo } from "@/lib/tauri"
import { ToolCard } from "@/components/settings/tool-card"

export default function ToolsPage() {
  const [toolList, setToolList] = React.useState<ToolInfo[]>([])
  const [loading, setLoading] = React.useState(true)

  React.useEffect(() => {
    const loadTools = async () => {
      try {
        const data = await tools.list()
        setToolList(data)
      } catch (error) {
        console.error("Failed to load tools:", error)
      } finally {
        setLoading(false)
      }
    }
    loadTools()
  }, [])

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <div className="text-muted-foreground">加载中...</div>
      </div>
    )
  }

  return (
    <div className="w-full space-y-4">
      <div>
        <h1 className="text-sm font-semibold">工具</h1>
        <p className="text-muted-foreground text-xs">
          系统中的所有可用工具
        </p>
      </div>

      {toolList.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-64 border rounded-lg border-dashed">
          <p className="text-muted-foreground">暂无可用工具</p>
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {toolList.map((tool) => (
            <ToolCard key={tool.name} tool={tool} />
          ))}
        </div>
      )}
    </div>
  )
}
