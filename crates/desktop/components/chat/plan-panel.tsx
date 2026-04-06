"use client";

import { useState } from "react";
import {
  CheckIcon,
  ChevronRightIcon,
  MinusIcon,
  LayoutList,
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
    <div className="w-full rounded-2xl border border-muted/60 bg-background/95 backdrop-blur-xl shadow-xl overflow-hidden animate-in slide-in-from-bottom-2 duration-300">
      {/* Header */}
      <div
        className="flex cursor-pointer items-center gap-2 select-none px-4 py-2.5 hover:bg-muted/30 transition-colors border-b border-muted/40 bg-muted/10"
        onClick={() => setCollapsed((c) => !c)}
      >
        <div className="bg-primary/10 p-1 rounded-md text-primary">
          <LayoutList className="size-3.5" />
        </div>
        <span className="text-xs font-bold uppercase tracking-widest text-foreground/80">
          执行计划
        </span>
        <span className="text-[10px] font-mono text-muted-foreground bg-muted px-1.5 py-0.5 rounded-full">
          {completed} / {total}
        </span>
        <div className="ml-auto flex items-center gap-1">
          <button
            className="rounded-full p-1 text-muted-foreground hover:bg-muted transition-colors"
            onClick={(e) => {
              e.stopPropagation();
              setCollapsed(!collapsed);
            }}
            aria-label={collapsed ? "展开" : "折叠"}
          >
            {collapsed ? (
              <ChevronRightIcon className="size-3.5" />
            ) : (
              <MinusIcon className="size-3.5" />
            )}
          </button>
        </div>
      </div>

      {/* Steps list - Fixed height & Scrollable */}
      {!collapsed && (
        <div className="max-h-[160px] overflow-y-auto custom-scrollbar px-4 py-3 bg-background/50">
          <ul className="space-y-2.5">
            {plan.map((item, i) => (
              <li key={i} className="flex items-start gap-2.5 text-xs animate-in fade-in duration-300">
                <StatusIcon status={item.status} />
                <span
                  className={cn(
                    "leading-relaxed break-words flex-1",
                    item.status === "pending" && "text-muted-foreground/60",
                    item.status === "in_progress" && "text-foreground font-semibold",
                    item.status === "completed" && "text-muted-foreground/50 line-through decoration-muted-foreground/30",
                  )}
                >
                  {item.step}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function StatusIcon({ status }: { status: PlanItem["status"] }) {
  if (status === "completed") {
    return (
      <div className="bg-emerald-100 dark:bg-emerald-900/30 p-0.5 rounded-full mt-0.5 shrink-0">
        <CheckIcon className="size-3 text-emerald-600 dark:text-emerald-400" />
      </div>
    );
  }
  if (status === "in_progress") {
    return (
      <div className="mt-1 flex size-3.5 shrink-0 items-center justify-center">
        <span className="relative flex size-2.5 items-center justify-center">
          <span className="absolute inline-flex size-full animate-ping rounded-full bg-primary/40 opacity-75" />
          <span className="relative inline-flex size-2 rounded-full bg-primary" />
        </span>
      </div>
    );
  }
  // pending
  return (
    <div className="mt-1 size-3.5 shrink-0 rounded-full border-2 border-muted-foreground/20" />
  );
}
