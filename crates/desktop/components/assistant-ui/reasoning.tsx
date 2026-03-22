"use client";

import { AuiIf, MessagePartPrimitive, useMessagePartText } from "@assistant-ui/react";
import { ChevronDownIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { FC } from "react";

const ReasoningBlock: FC = () => {
  const [isOpen, setIsOpen] = useState(false);
  const reasoning = useMessagePartText();
  const contentRef = useRef<HTMLDivElement>(null);
  const prevTextLengthRef = useRef(0);

  // Auto-scroll during streaming
  useEffect(() => {
    const el = contentRef.current;
    if (!el) return;

    const currentLength = reasoning.text.length;
    if (currentLength > prevTextLengthRef.current) {
      el.scrollTop = el.scrollHeight;
    }
    prevTextLengthRef.current = currentLength;
  }, [reasoning.text]);

  return (
    <div className="aui-reasoning-block mb-2 text-sm">
      <details className="group w-full" open={isOpen}>
        <summary
          className="flex w-full cursor-pointer list-none items-center gap-2 rounded-md px-1 py-1 text-muted-foreground transition-colors hover:bg-muted/30 [&::-webkit-details-marker]:hidden"
          onClick={(e) => {
            e.preventDefault();
            setIsOpen(!isOpen);
          }}
        >
          <MessagePartPrimitive.InProgress>
            <>
              <span className="relative flex size-2 items-center justify-center">
                <span className="absolute inline-flex size-full animate-ping rounded-full bg-primary/40 opacity-75"></span>
                <span className="relative inline-flex size-2 rounded-full bg-primary/60"></span>
              </span>
              <span className="opacity-70">思考中...</span>
            </>
          </MessagePartPrimitive.InProgress>
          <AuiIf condition={(s) => s.part.status.type !== "running"}>
            <>
              <span className="size-2 rounded-full bg-primary/40"></span>
              <span className="opacity-70">思考完成</span>
            </>
          </AuiIf>
          <ChevronDownIcon className="ml-auto size-4 shrink-0 opacity-50 transition-transform duration-200 group-open:rotate-180" />
        </summary>
        <div
          ref={contentRef}
          className="max-h-[150px] overflow-y-auto px-1 py-1 text-muted-foreground"
        >
          {reasoning.text}
        </div>
      </details>
    </div>
  );
};

export { ReasoningBlock as Reasoning };
