// crates/desktop/src/components/chat/markdown/MarkdownText.tsx

import { memo, type ReactElement } from "react";
import { MarkdownTextPrimitive } from "@assistant-ui/react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import "katex/dist/katex.min.css";

import { CodeBlock } from "./CodeBlock";
import { MermaidBlock } from "./MermaidBlock";

interface MarkdownTextProps {
  content: string;
}

/**
 * 自定义 Markdown 渲染组件
 * - 支持 GFM (GitHub Flavored Markdown)
 * - 支持数学公式 (KaTeX)
 * - 支持代码高亮 (Shiki)
 * - 支持 Mermaid 图表
 */
export const MarkdownText = memo(function MarkdownText({ content }: MarkdownTextProps) {
  return (
    <MarkdownTextPrimitive
      content={content}
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeKatex]}
      components={{
        // 自定义代码块处理
        pre: ({ children, ...props }) => {
          const codeElement = children as ReactElement<{
            className?: string;
            children?: string | string[];
          }>;

          const className = codeElement?.props?.className || "";

          // 处理 children 可能是数组或字符串的情况
          let codeContent = "";
          const rawChildren = codeElement?.props?.children;
          if (Array.isArray(rawChildren)) {
            codeContent = rawChildren.join("");
          } else if (typeof rawChildren === "string") {
            codeContent = rawChildren;
          } else if (rawChildren != null) {
            codeContent = String(rawChildren);
          }

          // 检测是否是 mermaid 代码块
          if (className.includes("language-mermaid")) {
            return <MermaidBlock code={codeContent} />;
          }

          // 普通代码块
          return (
            <CodeBlock className={className}>
              {codeContent}
            </CodeBlock>
          );
        },
      }}
    />
  );
});
