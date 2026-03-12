// crates/desktop/src/components/chat/markdown/MermaidBlock.tsx

import { useState, useEffect, useRef } from "react";
import mermaid from "mermaid";

interface MermaidBlockProps {
  code: string;
}

// 全局 ID 计数器
let mermaidIdCounter = 0;

// 初始化 mermaid（模块级别，只执行一次）
mermaid.initialize({
  startOnLoad: false,
  theme: "default",
  securityLevel: "loose",
});

/**
 * Mermaid 图表渲染组件
 */
export function MermaidBlock({ code }: MermaidBlockProps) {
  const [svg, setSvg] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!code) {
      setSvg("");
      setError(null);
      return;
    }

    let cancelled = false;

    const renderDiagram = async () => {
      try {
        // 根据当前主题更新 mermaid 配置
        const isDark = document.documentElement.classList.contains("dark");
        mermaid.initialize({
          startOnLoad: false,
          theme: isDark ? "dark" : "default",
          securityLevel: "loose",
        });

        // 生成唯一 ID
        const id = `mermaid-${++mermaidIdCounter}`;

        // 渲染图表
        const { svg: renderedSvg } = await mermaid.render(id, code);

        if (!cancelled) {
          setSvg(renderedSvg);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) {
          const errorMessage = err instanceof Error ? err.message : "Mermaid 渲染失败";
          setError(errorMessage);
          setSvg("");
        }
      }
    };

    renderDiagram();

    return () => {
      cancelled = true;
    };
  }, [code]);

  if (error) {
    return (
      <div className="my-4 p-4 rounded-lg bg-destructive/10 text-destructive border border-destructive/20">
        <p className="font-medium text-sm">Mermaid 图表渲染失败</p>
        <p className="text-xs mt-1 opacity-80">{error}</p>
        <pre className="mt-2 text-xs bg-muted/50 p-2 rounded overflow-x-auto">
          {code}
        </pre>
      </div>
    );
  }

  if (!svg) {
    return (
      <div className="my-4 p-4 rounded-lg bg-muted/50 animate-pulse">
        <div className="h-20 bg-muted rounded" />
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className="my-4 p-4 rounded-lg bg-muted/30 overflow-x-auto flex justify-center"
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}
