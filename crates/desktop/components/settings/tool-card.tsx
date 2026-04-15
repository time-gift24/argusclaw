"use client"

import * as React from "react"
import { ChevronDown } from "lucide-react"
import { type ToolInfo } from "@/lib/tauri"
import { Card, CardContent, CardHeader } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"

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
    <Card className="overflow-hidden">
      <CardHeader className="p-3 pb-2">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <h3 className="text-sm font-semibold">{tool.name}</h3>
            <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
              {tool.description}
            </p>
          </div>
          <Badge className={riskColors[tool.risk_level]} variant="secondary">
            {tool.risk_level}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="p-3 pt-0">
        <Collapsible open={showParams} onOpenChange={setShowParams}>
          <CollapsibleTrigger className="group/trigger flex w-full items-center justify-between rounded-md border border-muted/50 bg-muted/20 px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted/40 hover:text-foreground">
            <span>{showParams ? "隐藏" : "显示"}参数 schema</span>
            <ChevronDown className="h-3.5 w-3.5 transition-transform group-data-[panel-open]/trigger:rotate-180" />
          </CollapsibleTrigger>
          <CollapsibleContent className="overflow-hidden data-[closed]:animate-collapsible-up data-[open]:animate-collapsible-down">
          <pre
            id={`tool-params-${tool.name}`}
              className="mt-2 max-h-56 overflow-auto rounded-md bg-muted p-2 font-mono text-[11px] custom-scrollbar"
          >
            {JSON.stringify(tool.parameters, null, 2)}
          </pre>
          </CollapsibleContent>
        </Collapsible>
      </CardContent>
    </Card>
  )
}
