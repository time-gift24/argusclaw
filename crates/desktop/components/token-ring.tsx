import type { FC } from "react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

export const TokenRing: FC<{
  modelContextWindow: number;
  tokenCount?: number;
  className?: string;
}> = ({ modelContextWindow, tokenCount = 0, className }) => {
  const percentage = modelContextWindow > 0
    ? Math.min((tokenCount / modelContextWindow) * 100, 100)
    : 0;

  const radius = 11;
  const strokeWidth = 3;
  const circumference = 2 * Math.PI * radius;
  const strokeDashoffset = circumference - (percentage / 100) * circumference;

  const color = percentage > 80 ? "text-destructive" : percentage > 60 ? "text-amber-500" : "text-emerald-500";

  return (
    <Tooltip>
      <TooltipTrigger render={
        <div className={className}>
          <svg
            width={radius * 2 + strokeWidth * 2}
            height={radius * 2 + strokeWidth * 2}
            viewBox={`0 0 ${radius * 2 + strokeWidth * 2} ${radius * 2 + strokeWidth * 2}`}
            className="-rotate-90"
          >
            {/* Background track */}
            <circle
              cx={radius + strokeWidth}
              cy={radius + strokeWidth}
              r={radius}
              fill="none"
              stroke="currentColor"
              strokeWidth={strokeWidth}
              className="text-muted opacity-40"
            />
            {/* Progress arc */}
            <circle
              cx={radius + strokeWidth}
              cy={radius + strokeWidth}
              r={radius}
              fill="none"
              stroke="currentColor"
              strokeWidth={strokeWidth}
              strokeLinecap="round"
              strokeDasharray={circumference}
              strokeDashoffset={strokeDashoffset}
              className={color}
            />
          </svg>
        </div>
      } />
      <TooltipContent side="top">
        {`${tokenCount} / ${modelContextWindow} tokens`}
      </TooltipContent>
    </Tooltip>
  );
};
