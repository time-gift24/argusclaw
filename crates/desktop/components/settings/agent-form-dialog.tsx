"use client"

import * as React from "react"
import { Plus, Pencil } from "lucide-react"
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
import { LlmProviderSummary } from "./provider-card"

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

interface AgentFormDialogProps {
  agent?: AgentRecord | null
  providers: LlmProviderSummary[]
  onSubmit: (record: AgentRecord) => Promise<void>
  trigger?: React.ReactElement
}

export function AgentFormDialog({ agent, providers, onSubmit, trigger }: AgentFormDialogProps) {
  const [open, setOpen] = React.useState(false)
  const [loading, setLoading] = React.useState(false)
  const isEditing = !!agent

  const [formData, setFormData] = React.useState<AgentRecord>(() => {
    if (agent) {
      return agent
    }
    return {
      id: "",
      display_name: "",
      description: "",
      version: "1.0.0",
      provider_id: providers.find((p) => p.is_default)?.id || providers[0]?.id || "",
      system_prompt: "",
      tool_names: [],
      max_tokens: undefined,
      temperature: undefined,
    }
  })

  React.useEffect(() => {
    if (agent) {
      setFormData(agent)
    } else {
      setFormData({
        id: "",
        display_name: "",
        description: "",
        version: "1.0.0",
        provider_id: providers.find((p) => p.is_default)?.id || providers[0]?.id || "",
        system_prompt: "",
        tool_names: [],
        max_tokens: undefined,
        temperature: undefined,
      })
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
      <DialogTrigger render={trigger ? trigger : defaultTrigger} />
      <DialogContent className="sm:max-w-lg max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{isEditing ? "Edit Agent" : "Add Agent"}</DialogTitle>
          <DialogDescription>
            {isEditing ? "Update the agent configuration." : "Configure a new agent."}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="id">ID</Label>
              <Input
                id="id"
                value={formData.id}
                onChange={(e) => setFormData({ ...formData, id: e.target.value })}
                placeholder="my-agent"
                required
                disabled={isEditing}
              />
            </div>
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
          </div>
          <div className="space-y-2">
            <Label htmlFor="description">Description</Label>
            <Input
              id="description"
              value={formData.description}
              onChange={(e) => setFormData({ ...formData, description: e.target.value })}
              placeholder="A helpful agent"
            />
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="version">Version</Label>
              <Input
                id="version"
                value={formData.version}
                onChange={(e) => setFormData({ ...formData, version: e.target.value })}
                placeholder="1.0.0"
                required
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="provider_id">Provider</Label>
              <select
                id="provider_id"
                value={formData.provider_id}
                onChange={(e) => setFormData({ ...formData, provider_id: e.target.value })}
                className="flex h-7 w-full rounded-md border border-input bg-input/20 px-2 py-0.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/30 dark:bg-input/30"
                required
              >
                <option value="">Select a provider</option>
                {providers.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.display_name} {p.is_default ? "(Default)" : ""}
                  </option>
                ))}
              </select>
            </div>
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
