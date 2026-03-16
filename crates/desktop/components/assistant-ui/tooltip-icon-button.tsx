"use client";

import { forwardRef } from "react";
import { Slot } from "radix-ui";
import { type VariantProps } from "class-variance-authority";

import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { Button, buttonVariants } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export type TooltipIconButtonProps = VariantProps<typeof buttonVariants> & {
  tooltip: string;
  side?: "top" | "bottom" | "left" | "right";
  className?: string;
  children?: React.ReactNode;
  "aria-label"?: string;
  onClick?: React.MouseEventHandler<HTMLButtonElement>;
  disabled?: boolean;
  type?: "button" | "submit" | "reset";
};

export const TooltipIconButton = forwardRef<
  HTMLButtonElement,
  TooltipIconButtonProps
>(({ children, tooltip, side = "bottom", variant = "ghost", size = "icon", type, className, ...rest }, ref) => {
  return (
    <Tooltip>
      <TooltipTrigger render={<Button variant={variant} size={size} type={type} className={cn("aui-button-icon size-6 p-1", className)} ref={ref} {...rest} />}><Slot.Slottable>{children}</Slot.Slottable><span className="aui-sr-only sr-only">{tooltip}</span></TooltipTrigger>
      <TooltipContent side={side}>{tooltip}</TooltipContent>
    </Tooltip>
  );
});

TooltipIconButton.displayName = "TooltipIconButton";
