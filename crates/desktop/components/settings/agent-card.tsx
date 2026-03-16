"use client"

import { Bot, Pencil, Trash2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

export interface AgentRecord {
  id: string
  display_name: string
  description: string
  version: string
  provider_id: string
  system_prompt: string
  tool_names: string[]
  max_tokens?: number
  temperature?: number
}

interface AgentCardProps {
  agent: AgentRecord
  onEdit: (id: string) => void
  onDelete: (id: string) => void
}

export function AgentCard({ agent, onEdit, onDelete }: AgentCardProps) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Bot className="h-5 w-5 text-muted-foreground" />
            <CardTitle className="text-base">{agent.display_name}</CardTitle>
          </div>
          <Badge variant="outline" className="text-xs">
            v{agent.version}
          </Badge>
        </div>
        <CardDescription className="text-xs">{agent.id}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-2 text-sm">
        <p className="text-muted-foreground line-clamp-2">{agent.description}</p>
        <div className="flex justify-between">
          <span className="text-muted-foreground">Provider:</span>
          <span className="font-mono text-xs">{agent.provider_id}</span>
        </div>
        {agent.tool_names.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {agent.tool_names.map((tool) => (
              <Badge key={tool} variant="secondary" className="text-xs">
                {tool}
              </Badge>
            ))}
          </div>
        )}
        <div className="flex gap-4 text-xs text-muted-foreground">
          {agent.max_tokens && <span>Max tokens: {agent.max_tokens}</span>}
          {agent.temperature !== undefined && <span>Temp: {agent.temperature}</span>}
        </div>
      </CardContent>
      <CardFooter className="gap-2">
        <Button size="sm" variant="outline" onClick={() => onEdit(agent.id)}>
          <Pencil className="h-3 w-3 mr-1" />
          Edit
        </Button>
        <Button size="sm" variant="destructive" onClick={() => onDelete(agent.id)}>
          <Trash2 className="h-3 w-3 mr-1" />
          Delete
        </Button>
      </CardFooter>
    </Card>
  )
}
