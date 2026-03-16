"use client"

import * as React from "react"

import { cn } from "@/lib/utils"

type CheckboxProps = Omit<React.ComponentProps<"input">, "type"> & {
  onCheckedChange?: (checked: boolean) => void
}

function Checkbox({
  className,
  onChange,
  onCheckedChange,
  ...props
}: CheckboxProps) {
  return (
    <input
      type="checkbox"
      data-slot="checkbox"
      className={cn(
        "size-4 shrink-0 rounded-sm border border-input bg-input/20 accent-primary outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring/30 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-input/30",
        className,
      )}
      onChange={(event) => {
        onChange?.(event)
        onCheckedChange?.(event.currentTarget.checked)
      }}
      {...props}
    />
  )
}

export { Checkbox }
