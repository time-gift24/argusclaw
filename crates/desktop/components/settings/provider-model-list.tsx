"use client";

import * as React from "react";
import { Plus } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

interface ProviderModelListProps {
  models: string[];
  defaultModel: string;
  onModelsChange: (models: string[]) => void;
  onDefaultModelChange: (model: string) => void;
  disabled?: boolean;
}

export function ProviderModelList({
  models,
  defaultModel,
  onModelsChange,
  onDefaultModelChange,
  disabled = false,
}: ProviderModelListProps) {
  const [newModel, setNewModel] = React.useState("");

  const handleAddModel = React.useCallback(() => {
    const trimmed = newModel.trim();
    if (!trimmed || models.includes(trimmed)) return;
    const newModels = [...models, trimmed];
    onModelsChange(newModels);
    if (!defaultModel) {
      onDefaultModelChange(trimmed);
    }
    setNewModel("");
  }, [newModel, models, defaultModel, onModelsChange, onDefaultModelChange]);

  const handleRemoveModel = React.useCallback(
    (model: string) => {
      const newModels = models.filter((m) => m !== model);
      onModelsChange(newModels);
      if (defaultModel === model) {
        onDefaultModelChange(newModels[0] || "");
      }
    },
    [models, defaultModel, onModelsChange, onDefaultModelChange]
  );

  const handleKeyDown = React.useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleAddModel();
      }
    },
    [handleAddModel]
  );

  return (
    <div className="space-y-2">
      <label className="text-xs text-muted-foreground">模型列表</label>
      {models.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {models.map((model) => (
            <Badge
              key={model}
              variant={model === defaultModel ? "default" : "secondary"}
              className="cursor-pointer pr-1"
              onClick={() => onDefaultModelChange(model)}
            >
              {model}
              {model === defaultModel && (
                <span className="ml-1 text-[10px] opacity-70">默认</span>
              )}
              {!disabled && (
                <button
                  type="button"
                  className="ml-1 hover:text-destructive"
                  onClick={(e) => {
                    e.stopPropagation();
                    handleRemoveModel(model);
                  }}
                >
                  ×
                </button>
              )}
            </Badge>
          ))}
        </div>
      )}
      {!disabled && (
        <div className="flex gap-2">
          <Input
            value={newModel}
            onChange={(e) => setNewModel(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="输入模型名称"
            disabled={disabled}
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={handleAddModel}
            disabled={!newModel.trim() || disabled}
          >
            <Plus className="h-4 w-4" />
          </Button>
        </div>
      )}
      <p className="text-[11px] text-muted-foreground">点击标签设为默认模型</p>
    </div>
  );
}
