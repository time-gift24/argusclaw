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

export interface LlmProviderRecord {
  id: string
  kind: "openai-compatible"
  display_name: string
  base_url: string
  api_key: string
  model: string
  is_default: boolean
  extra_headers: Record<string, string>
}

interface ProviderFormDialogProps {
  provider?: LlmProviderRecord | null
  onSubmit: (record: LlmProviderRecord) => Promise<void>
  open?: boolean
  onOpenChange?: (open: boolean) => void
  trigger?: React.ReactElement | null
}

export function ProviderFormDialog({
  provider,
  onSubmit,
  open: openProp,
  onOpenChange,
  trigger,
}: ProviderFormDialogProps) {
  const [internalOpen, setInternalOpen] = React.useState(false)
  const [loading, setLoading] = React.useState(false)
  const isEditing = !!provider
  const open = openProp ?? internalOpen

  const handleOpenChange = React.useCallback(
    (nextOpen: boolean) => {
      if (openProp === undefined) {
        setInternalOpen(nextOpen)
      }
      onOpenChange?.(nextOpen)
    },
    [onOpenChange, openProp],
  )

  const [formData, setFormData] = React.useState<LlmProviderRecord>(() =>
    provider || {
      id: "",
      kind: "openai-compatible",
      display_name: "",
      base_url: "",
      api_key: "",
      model: "",
      is_default: false,
      extra_headers: {},
    }
  )

  React.useEffect(() => {
    if (provider) {
      setFormData(provider)
    } else {
      setFormData({
        id: "",
        kind: "openai-compatible",
        display_name: "",
        base_url: "",
        api_key: "",
        model: "",
        is_default: false,
        extra_headers: {},
      })
    }
  }, [provider])
  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoading(true)
    try {
      await onSubmit(formData)
      handleOpenChange(false)
    } catch (error) {
      console.error("Failed to save provider:", error)
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
      Add Provider
    </Button>
  )
  const dialogTrigger = trigger === undefined ? defaultTrigger : trigger

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      {dialogTrigger ? <DialogTrigger render={dialogTrigger} /> : null}
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{isEditing ? "Edit Provider" : "Add Provider"}</DialogTitle>
          <DialogDescription>
            {isEditing
              ? "Update the LLM provider configuration."
              : "Configure a new LLM provider."}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="id">ID</Label>
            <Input
              id="id"
              value={formData.id}
              onChange={(e) => setFormData({ ...formData, id: e.target.value })}
              placeholder="unique-provider-id"
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
              placeholder="My LLM Provider"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="base_url">Base URL</Label>
            <Input
              id="base_url"
              value={formData.base_url}
              onChange={(e) => setFormData({ ...formData, base_url: e.target.value })}
              placeholder="https://api.example.com/v1"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="api_key">API Key</Label>
            <Input
              id="api_key"
              type="password"
              value={formData.api_key}
              onChange={(e) => setFormData({ ...formData, api_key: e.target.value })}
              placeholder="sk-..."
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="model">Model</Label>
            <Input
              id="model"
              value={formData.model}
              onChange={(e) => setFormData({ ...formData, model: e.target.value })}
              placeholder="gpt-4"
              required
            />
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
