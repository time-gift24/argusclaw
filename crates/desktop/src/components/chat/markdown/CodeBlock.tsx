// crates/desktop/src/components/chat/markdown/CodeBlock.tsx

import { useState, useEffect, useCallback, useMemo, type ReactNode } from "react";
import { CopyIcon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { codeToHtml, bundledThemes, type ThemeRegistration } from "shiki";

interface CodeBlockProps {
  className?: string;
  children?: ReactNode;
  language?: string;
  code?: string;
  isIncomplete?: boolean; // 流式输出时使用
}

// 背景色常量
const LIGHT_BG = "#f1f5f9";
const DARK_BG = "#1e1e1e";

// 主题缓存
let customLightTheme: ThemeRegistration | null = null;
let customDarkTheme: ThemeRegistration | null = null;

async function loadCustomThemes() {
  if (!customLightTheme) {
    const lightTheme = await bundledThemes["github-light"]().then((m) => m.default);
    customLightTheme = {
      ...lightTheme,
      bg: LIGHT_BG,
      colors: {
        ...lightTheme.colors,
        "editor.background": LIGHT_BG,
      },
    };
  }
  if (!customDarkTheme) {
    const darkTheme = await bundledThemes["github-dark"]().then((m) => m.default);
    customDarkTheme = {
      ...darkTheme,
      bg: DARK_BG,
      colors: {
        ...darkTheme.colors,
        "editor.background": DARK_BG,
      },
    };
  }
  return { light: customLightTheme!, dark: customDarkTheme! };
}

// 预加载主题
loadCustomThemes();

// 语言别名映射
const LANG_MAP: Record<string, string> = {
  ts: "typescript",
  js: "javascript",
  py: "python",
  rb: "ruby",
  sh: "bash",
  shell: "bash",
  yml: "yaml",
};

/**
 * 从 className 提取语言
 */
function extractLanguage(className: string): string {
  const match = className.match(/language-(\w+)/);
  return match ? match[1].toLowerCase() : "text";
}

/**
 * 提取子节点文本
 */
function extractTextFromChildren(children: ReactNode): string {
  if (typeof children === "string") return children;
  if (typeof children === "number") return String(children);
  if (!children) return "";

  if (Array.isArray(children)) {
    return children.map(extractTextFromChildren).join("");
  }

  if (typeof children === "object" && "props" in children) {
    return extractTextFromChildren(
      (children as ReactNode & { props?: { children?: ReactNode } }).props?.children
    );
  }

  return "";
}

/**
 * 检测当前主题
 */
function useTheme() {
  const [isDark, setIsDark] = useState(false);

  useEffect(() => {
    const checkTheme = () => {
      setIsDark(document.documentElement.classList.contains("dark"));
    };

    checkTheme();

    const observer = new MutationObserver(checkTheme);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => observer.disconnect();
  }, []);

  return isDark;
}

/**
 * HTML 转义
 */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

/**
 * 代码块组件
 * - Shiki 语法高亮
 * - 明暗主题支持
 * - 复制功能
 */
export function CodeBlock({ className, children, language: langProp, code: rawCode, isIncomplete }: CodeBlockProps) {
  const [highlightedHtml, setHighlightedHtml] = useState("");
  const [copied, setCopied] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const isDark = useTheme();

  // 提取语言
  const language = langProp || extractLanguage(className || "");

  // 提取代码文本
  const codeText = useMemo(() => {
    if (rawCode) return rawCode;
    return extractTextFromChildren(children);
  }, [rawCode, children]);

  // 背景色
  const codeBg = isDark ? DARK_BG : LIGHT_BG;

  // 语法高亮
  useEffect(() => {
    if (!codeText) {
      setHighlightedHtml("");
      setIsLoading(false);
      return;
    }

    const highlight = async () => {
      const mappedLang = LANG_MAP[language] || language || "text";

      try {
        const themes = await loadCustomThemes();
        const html = await codeToHtml(codeText, {
          lang: mappedLang,
          themes: {
            light: themes.light,
            dark: themes.dark,
          },
          defaultColor: isDark ? "dark" : "light",
        });
        setHighlightedHtml(html);
      } catch (err) {
        console.warn("Shiki highlighting failed:", err);
        // 回退到纯文本
        setHighlightedHtml(`<pre style="margin:0"><code>${escapeHtml(codeText)}</code></pre>`);
      } finally {
        setIsLoading(false);
      }
    };

    highlight();
  }, [codeText, language, isDark]);

  // 复制功能
  const handleCopy = useCallback(async () => {
    if (!codeText) return;

    try {
      await navigator.clipboard.writeText(codeText);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error("Failed to copy:", err);
    }
  }, [codeText]);

  // 流式输出或加载中显示骨架屏
  if (isIncomplete || isLoading) {
    return (
      <div className="my-3 rounded-lg overflow-hidden">
        <div
          className="flex items-center justify-between px-2 py-1 border-b"
          style={{ backgroundColor: codeBg, borderColor: "var(--border, #e2e8f0)" }}
        >
          <span
            className="text-[12px] font-medium"
            style={{ color: "var(--muted-foreground, #64748b)" }}
          >
            {language || "code"}
          </span>
        </div>
        <div className="p-3" style={{ backgroundColor: codeBg }}>
          <div className="animate-pulse h-4 w-3/4 rounded" style={{ backgroundColor: "var(--muted, #e2e8f0)" }} />
        </div>
      </div>
    );
  }

  if (!codeText) {
    return null;
  }

  return (
    <div className="my-3 rounded-lg overflow-hidden">
      {/* Header */}
      <div
        className="flex items-center justify-between px-2 py-1 border-b"
        style={{ backgroundColor: codeBg, borderColor: "var(--border, #e2e8f0)" }}
      >
        <span
          className="text-[12px] font-medium"
          style={{ color: "var(--muted-foreground, #64748b)" }}
        >
          {language || "code"}
        </span>
        <button
          onClick={handleCopy}
          className="p-1 rounded hover:opacity-80 transition-opacity"
          style={{ color: "var(--muted-foreground, #64748b)" }}
          title={copied ? "已复制!" : "复制代码"}
        >
          <HugeiconsIcon icon={CopyIcon} className="w-3.5 h-3.5" />
        </button>
      </div>

      {/* Code content */}
      <div
        className="p-3 overflow-x-auto"
        style={{ backgroundColor: codeBg, fontSize: "var(--sd-code-font-size, 13px)" }}
      >
        <code
          dangerouslySetInnerHTML={{ __html: highlightedHtml || escapeHtml(codeText) }}
        />
      </div>
    </div>
  );
}
