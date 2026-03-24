import type { FC } from "react";

export const TokenRing: FC<{
  modelContextWindow: number;
  tokenCount?: number;
  className?: string;
}> = ({ modelContextWindow, tokenCount = 0, className }) => {
  const percentage = modelContextWindow > 0
    ? Math.min((tokenCount / modelContextWindow) * 100, 100)
    : 0;

  const radius = 12;
  const strokeWidth = 2.5;
  const circumference = 2 * Math.PI * radius;
  const strokeDashoffset = circumference - (percentage / 100) * circumference;

  return (
    <div className={className} title={`${tokenCount} / ${modelContextWindow} tokens`}>
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
          className="text-muted opacity-20"
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
          className={percentage > 80 ? "text-destructive" : "text-primary"}
        />
      </svg>
    </div>
  );
};
