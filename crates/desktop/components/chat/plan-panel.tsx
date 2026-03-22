"use client";

import { useState } from "react";
import {
  CheckIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  MinusIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { PlanItem } from "@/lib/types/plan";

interface PlanPanelProps {
  plan: PlanItem[];
}

export function PlanPanel({ plan }: PlanPanelProps) {
  const [collapsed, setCollapsed] = useState(false);

  const completed = plan.filter((item) => item.status === "completed").length;
  const total = plan.length;

  return (
    <div className="mb-2 w-full rounded-lg border border-border/60 bg-muted/30 px-3 py-2">
      {/* Header */}
      <div
        className="flex cursor-pointer items-center gap-2 select-none"
        onClick={() => setCollapsed((c) => !c)}
      >
        {collapsed ? (
          <ChevronRightIcon className="size-4 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronDownIcon className="size-4 shrink-0 text-muted-foreground" />
        )}
        <span className="text-sm font-medium text-foreground">
          Plan
        </span>
        <span className="text-sm text-muted-foreground">
          ({completed}/{total})
        </span>
        <button
          className="ml-auto rounded p-0.5 text-muted-foreground hover:bg-muted"
          onClick={(e) => {
            e.stopPropagation();
            setCollapsed(true);
          }}
          aria-label="折叠"
        >
          <MinusIcon className="size-3" />
        </button>
      </div>

      {/* Steps list */}
      {!collapsed && (
        <ul className="mt-2 space-y-1.5 pl-6">
          {plan.map((item, i) => (
            <li key={i} className="flex items-start gap-2 text-sm">
              <StatusIcon status={item.status} />
              <span
                className={cn(
                  "leading-snug",
                  item.status === "pending" && "text-muted-foreground",
                  item.status === "in_progress" && "text-foreground font-medium",
                  item.status === "completed" && "text-muted-foreground line-through",
                )}
              >
                {item.step}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function StatusIcon({ status }: { status: PlanItem["status"] }) {
  if (status === "completed") {
    return (
      <CheckIcon className="size-3.5 mt-0.5 shrink-0 text-green-500" />
    );
  }
  if (status === "in_progress") {
    return (
      <span className="mt-0.5 flex size-3.5 shrink-0 items-center justify-center">
        <span className="relative flex size-2 items-center justify-center">
          <span className="absolute inline-flex size-full animate-pulse rounded-full bg-primary/40 opacity-75" />
          <span className="relative inline-flex size-2 rounded-full bg-primary/70" />
        </span>
      </span>
    );
  }
  // pending
  return (
    <span className="mt-0.5 size-3.5 shrink-0 rounded-full border border-muted-foreground/40" />
  );
}
