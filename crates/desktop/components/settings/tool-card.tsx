"use client"

import * as React from "react"
import { type ToolInfo } from "@/lib/tauri"
import { Card, CardContent, CardHeader } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

const riskColors: Record<ToolInfo["risk_level"], string> = {
  low: "bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-300",
  medium: "bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-300",
  high: "bg-orange-100 text-orange-800 dark:bg-orange-900 dark:text-orange-300",
  critical: "bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-300",
}

interface ToolCardProps {
  tool: ToolInfo
}

export function ToolCard({ tool }: ToolCardProps) {
  const [showParams, setShowParams] = React.useState(false)

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-start justify-between">
          <div>
            <h3 className="text-sm font-semibold">{tool.name}</h3>
            <p className="text-xs text-muted-foreground mt-1">{tool.description}</p>
          </div>
          <Badge className={riskColors[tool.risk_level]} variant="secondary">
            {tool.risk_level}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="pt-0">
        <button
          type="button"
          aria-expanded={showParams}
          aria-controls={`tool-params-${tool.name}`}
          onClick={() => setShowParams(!showParams)}
          className="text-xs text-muted-foreground hover:text-foreground transition-colors"
        >
          {showParams ? "隐藏" : "显示"}参数 schema
        </button>
        {showParams && (
          <pre
            id={`tool-params-${tool.name}`}
            className="mt-2 text-xs bg-muted p-2 rounded-md overflow-x-auto"
          >
            {JSON.stringify(tool.parameters, null, 2)}
          </pre>
        )}
      </CardContent>
    </Card>
  )
}
