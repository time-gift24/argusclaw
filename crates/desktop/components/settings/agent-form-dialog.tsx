"use client"

import * as React from "react"
import { Plus, Pencil } from "lucide-react"
import { agents, type AgentRecord, type LlmProviderSummary } from "@/lib/tauri"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"

interface AgentFormDialogProps {
  agent?: AgentRecord | null
  providers: LlmProviderSummary[]
  onSubmit: (record: AgentRecord) => Promise<void>
  trigger?: React.ReactElement
}

function createDefaultFormData(providers: LlmProviderSummary[]): AgentRecord {
  const defaultProvider = providers.find((p) => p.is_default)
  return {
    id: 0,
    display_name: "",
    description: "",
    version: "1.0.0",
    provider_id: defaultProvider?.id ?? null,
    system_prompt: "",
    tool_names: [],
    max_tokens: undefined,
    temperature: undefined,
  }
}

export function AgentFormDialog({ agent, providers, onSubmit, trigger }: AgentFormDialogProps) {
  const [open, setOpen] = React.useState(false)
  const [loading, setLoading] = React.useState(false)
  const isEditing = !!agent

  const [formData, setFormData] = React.useState<AgentRecord>(() => {
    if (agent) {
      return agent
    }
    return createDefaultFormData(providers)
  })

  React.useEffect(() => {
    if (agent) {
      setFormData(agent)
    } else {
      setFormData(createDefaultFormData(providers))
    }
  }, [agent, providers])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoading(true)
    try {
      await onSubmit(formData)
      setOpen(false)
    } catch (error) {
      console.error("Failed to save agent:", error)
    } finally {
      setLoading(false)
    }
  }

  const defaultTrigger = isEditing ? (
    <Button size="sm" variant="outline">
      <Pencil className="h-3 w-3" />
    </Button>
  ) : (
    <Button size="sm">
      <Plus className="h-4 w-4 mr-1" />
      Add Agent
    </Button>
  )

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      {trigger === undefined ? <DialogTrigger render={defaultTrigger} /> : trigger ? <DialogTrigger render={trigger} /> : null}
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{isEditing ? "Edit Agent" : "Add Agent"}</DialogTitle>
          <DialogDescription>
            {isEditing
              ? "Update the agent configuration."
              : "Configure a new agent."}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="display_name">Display Name</Label>
            <Input
              id="display_name"
              value={formData.display_name}
              onChange={(e) => setFormData({ ...formData, display_name: e.target.value })}
              placeholder="My Agent"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="description">Description</Label>
            <Input
              id="description"
              value={formData.description}
              onChange={(e) => setFormData({ ...formData, description: e.target.value })}
              placeholder="A helpful assistant"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="provider_id">Provider</Label>
            <select
              id="provider_id"
              value={formData.provider_id ?? ""}
              onChange={(e) => setFormData({ ...formData, provider_id: e.target.value ? parseInt(e.target.value) : null })}
              className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <option value="">No provider</option>
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.display_name} {p.is_default ? "(Default)" : ""}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-2">
            <Label htmlFor="system_prompt">System Prompt</Label>
            <Textarea
              id="system_prompt"
              value={formData.system_prompt}
              onChange={(e) => setFormData({ ...formData, system_prompt: e.target.value })}
              placeholder="You are a helpful assistant..."
              rows={4}
              required
            />
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="max_tokens">Max Tokens (optional)</Label>
              <Input
                id="max_tokens"
                type="number"
                value={formData.max_tokens || ""}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    max_tokens: e.target.value ? parseInt(e.target.value) : undefined,
                  })
                }
                placeholder="4096"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="temperature">Temperature (optional)</Label>
              <Input
                id="temperature"
                type="number"
                step="0.1"
                min="0"
                max="2"
                value={formData.temperature ?? ""}
                onChange={(e) =>
                  setFormData({
                    ...formData,
                    temperature: e.target.value ? parseFloat(e.target.value) : undefined,
                  })
                }
                placeholder="0.7"
              />
            </div>
          </div>
          <DialogFooter>
            <Button type="submit" disabled={loading}>
              {loading ? "Saving..." : isEditing ? "Update" : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
