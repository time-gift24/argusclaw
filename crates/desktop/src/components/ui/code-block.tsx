import { useState, useCallback, useEffect, useMemo, type ReactNode, type HTMLAttributes } from "react";
import { CopyIcon } from "@hugeicons/core-free-icons";
import { HugeiconsIcon } from "@hugeicons/react";
import { codeToHtml, bundledThemes, type ThemeRegistration } from "shiki";
import mermaid from "mermaid";

interface CodeBlockProps extends HTMLAttributes<HTMLDivElement> {
  className?: string;
  children?: ReactNode;
  language?: string;
  code?: string;
  isIncomplete?: boolean;
}

// Initialize mermaid
mermaid.initialize({
  startOnLoad: false,
  theme: "default",
  securityLevel: "loose",
});

// 背景色常量 - 与容器保持一致
const LIGHT_BG = "#f1f5f9";
const DARK_BG = "#1e1e1e";

// 预加载并自定义主题
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

// 启动时预加载主题
loadCustomThemes();

/**
 * Custom CodeBlock component for Streamdown
 * - Solid background color (no nested borders)
 * - Light/Dark mode support
 * - Copy functionality with Shiki syntax highlighting
 * - Mermaid diagram rendering
 * Used via plugins.renderers
 */
function CodeBlock({ className, children, language: langProp, code: rawCode, isIncomplete, ...props }: CodeBlockProps) {
  const [copied, setCopied] = useState(false);
  const [highlightedCode, setHighlightedCode] = useState("");
  const [mermaidSvg, setMermaidSvg] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isDark, setIsDark] = useState(false);

  // Use language from props (passed by renderer) or extract from className
  const language = (langProp || className?.replace("language-", "") || "").toLowerCase();

  // Get the raw text content
  const codeText = useMemo(() => {
    if (rawCode) return rawCode;
    if (!children) return "";

    if (typeof children === "string") {
      return children;
    }

    const extractText = (node: ReactNode): string => {
      if (typeof node === "string") return node;
      if (typeof node === "number") return String(node);
      if (!node) return "";
      if (Array.isArray(node)) {
        return node.map(extractText).join("");
      }
      if (typeof node === "object" && "props" in node) {
        return extractText((node as ReactNode & { props?: { children?: ReactNode } }).props?.children);
      }
      return "";
    };

    return extractText(children);
  }, [children, rawCode]);

  const isMermaid = language === "mermaid";

  // Detect theme on mount and when it changes
  useEffect(() => {
    const checkTheme = () => {
      const dark = document.documentElement.classList.contains("dark");
      setIsDark(dark);
    };

    checkTheme();

    // Listen for theme changes
    const observer = new MutationObserver(checkTheme);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });

    return () => observer.disconnect();
  }, []);

  // Background color based on theme - 与 Shiki 主题背景保持一致
  const codeBg = isDark ? DARK_BG : LIGHT_BG;

  // Handle mermaid diagram rendering
  useEffect(() => {
    if (!isMermaid || !codeText) {
      setMermaidSvg("");
      setIsLoading(false);
      return;
    }

    const renderDiagram = async () => {
      try {
        mermaid.initialize({
          startOnLoad: false,
          theme: isDark ? "dark" : "default",
          securityLevel: "loose",
        });

        const id = `mermaid-${Math.random().toString(36).substr(2, 9)}`;
        const { svg } = await mermaid.render(id, codeText);
        setMermaidSvg(svg);
      } catch (err) {
        console.error("Mermaid render error:", err);
        setMermaidSvg("");
      } finally {
        setIsLoading(false);
      }
    };

    renderDiagram();
  }, [codeText, isMermaid]);

  // Highlight code with Shiki (async)
  useEffect(() => {
    if (!codeText || isMermaid) {
      setHighlightedCode("");
      if (!isMermaid) setIsLoading(false);
      return;
    }

    const highlight = async () => {
      // Map common language aliases
      const langMap: Record<string, string> = {
        ts: "typescript",
        js: "javascript",
        py: "python",
        rb: "ruby",
        sh: "bash",
        shell: "bash",
        yml: "yaml",
      };
      const mappedLang = langMap[language] || language;

      try {
        // 确保主题已加载
        const themes = await loadCustomThemes();

        const html = await codeToHtml(codeText, {
          lang: mappedLang || "text",
          themes: {
            light: themes.light,
            dark: themes.dark,
          },
          defaultColor: isDark ? "dark" : "light",
        });
        setHighlightedCode(html);
      } catch (err) {
        console.warn("Shiki highlighting failed:", err);
        setHighlightedCode(codeText);
      } finally {
        setIsLoading(false);
      }
    };

    highlight();
  }, [codeText, language, isMermaid]);

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

  // Show loading state during streaming or highlighting
  if (isIncomplete || isLoading) {
    return (
      <div className="my-3 rounded-lg overflow-hidden">
        <div className="flex items-center justify-between px-2 py-1 border-b" style={{ backgroundColor: codeBg, borderColor: "var(--border, #e2e8f0)" }}>
          <span className="text-[12px] font-medium" style={{ color: "var(--muted-foreground, #64748b)" }}>
            {language || "code"}
          </span>
        </div>
        <div className="p-3" style={{ backgroundColor: codeBg }}>
          <div className="animate-pulse h-4 w-3/4 rounded" style={{ backgroundColor: "var(--muted, #e2e8f0)" }} />
        </div>
      </div>
    );
  }

  return (
    <div className="my-3 rounded-lg overflow-hidden">
      {/* Header with language and copy button */}
      <div className="flex items-center justify-between px-2 py-1 border-b" style={{ backgroundColor: codeBg, borderColor: "var(--border, #e2e8f0)" }}>
        <span className="text-[12px] font-medium" style={{ color: "var(--muted-foreground, #64748b)" }}>
          {language || "code"}
        </span>
        <button
          onClick={handleCopy}
          className="p-1 rounded hover:opacity-80 transition-opacity"
          style={{ color: "var(--muted-foreground, #64748b)" }}
          title={copied ? "Copied!" : "Copy code"}
        >
          <HugeiconsIcon
            icon={CopyIcon}
            className="w-2.5 h-2.5"
          />
        </button>
      </div>

      {/* Code content - show mermaid diagram or syntax highlighted code */}
      <div
        className="p-3 overflow-x-auto"
        style={{ backgroundColor: codeBg, fontSize: "var(--sd-code-font-size, 13px)" }}
        {...props}
      >
        {isMermaid && mermaidSvg ? (
          <div dangerouslySetInnerHTML={{ __html: mermaidSvg }} />
        ) : (
          <code dangerouslySetInnerHTML={{ __html: highlightedCode || codeText }} />
        )}
      </div>
    </div>
  );
}

export { CodeBlock };
