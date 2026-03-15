"use client"

import * as React from "react"
import { Cloud, Pencil, Trash2, Check } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"

export interface LlmProviderSummary {
  id: string
  kind: string
  display_name: string
  base_url: string
  model: string
  is_default: boolean
  extra_headers: Record<string, string>
}

interface ProviderCardProps {
  provider: LlmProviderSummary
  onEdit: (id: string) => void
  onDelete: (id: string) => void
  onSetDefault: (id: string) => void
}

export function ProviderCard({ provider, onEdit, onDelete, onSetDefault }: ProviderCardProps) {
  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-base flex items-center gap-2">
            <Cloud className="h-5 w-5 text-muted-foreground" />
            <span>{provider.display_name}</span>
            {provider.is_default && (
              <Badge variant="default" className="bg-primary text-primary-foreground text-xs">
                <Check className="mr-1 h-3 w-3" />
                Default
              </Badge>
            )}
          </CardTitle>
          <CardDescription className="text-xs">{provider.id}</CardDescription>
        </div>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <div className="flex justify-between">
          <span className="text-muted-foreground">Kind:</span>
          <span className="font-mono text-xs bg-muted px-2 py-1 rounded">
            {provider.kind}
          </span>
        </div>
        <div className="flex justify-between">
          <span className="text-muted-foreground">Model:</span>
          <span className="font-mono text-xs break-all bg-muted px-2 py-1 rounded">
            {provider.model}
          </span>
        </div>
        <div className="flex justify-between">
          <span className="text-muted-foreground">Base URL:</span>
          <span className="font-mono text-xs break-all bg-muted px-2 py-1 rounded max-w-[200px]">
            {provider.base_url}
          </span>
        </div>
      </CardContent>
      <CardFooter className="gap-2">
        <Button size="sm" variant="outline" onClick={() => onEdit(provider.id)}>
          <Pencil className="h-3 w-3 mr-1" />
          Edit
        </Button>
        <Button size="sm" variant="outline" onClick={() => onSetDefault(provider.id)}>
          <Check className="h-3 w-3 mr-1" />
          Set Default
        </Button>
        <Button size="sm" variant="destructive" onClick={() => onDelete(provider.id)} disabled={provider.is_default}>
          <Trash2 className="h-3 w-3 mr-1" />
          Delete
        </Button>
      </CardFooter>
    </Card>
  )
}
