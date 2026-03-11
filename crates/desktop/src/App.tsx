import "./streamdown.css";

import React from "react";
import {
  Sidebar,
  SidebarContent,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { HugeiconsIcon } from "@hugeicons/react";
import { ChatIcon, CodeIcon, AiMagicIcon } from "@hugeicons/core-free-icons";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Streamdown } from "streamdown";
import { math } from "@streamdown/math";
import { cjk } from "@streamdown/cjk";
import { CodeBlock } from "@/components/ui/code-block";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

// 页面类型
type Page = "chat" | "streamdown-dev";

// 完整的测试 Markdown 内容
const SAMPLE_MARKDOWN = `# ArgusClaw AI 助手

欢迎使用 ArgusClaw！这是一个强大的 AI 助手，可以帮助你完成各种任务。

## 功能特点

- **智能对话**: 基于先进的 LLM 技术
- **代码支持**: 支持多种编程语言的语法高亮
- **数学公式**: 完美的 LaTeX 渲染
- **图表绘制**: 支持 Mermaid 流程图

## 代码示例

\`\`\`typescript
interface Message {
  id: string;
  role: "user" | "assistant";
  content: string;
  timestamp: Date;
}

function processMessage(msg: Message): string {
  return \`[\${msg.role}] \${msg.content}\`;
}
\`\`\`

## 数学公式

行内公式: $E = mc^2$

独立公式:
$$
f(x) = \\int_{-\\infty}^{\\infty} \\hat{f}(\\xi) e^{2\\pi i \\xi x} d\\xi
$$

## Mermaid 流程图

\`\`\`mermaid
graph TD
    A[用户输入] --> B{验证}
    B -->|通过| C[处理请求]
    B -->|失败| D[返回错误]
    C --> E[调用 LLM]
    E --> F[返回结果]
\`\`\`

## 表格

| 功能 | 状态 | 说明 |
|------|------|------|
| 对话 | ✅ | 支持流式输出 |
| 代码高亮 | ✅ | Shiki 引擎 |
| 数学公式 | ✅ | KaTeX |
| 图表 | ✅ | Mermaid |

## 列表

1. 第一步: 准备数据
2. 第二步: 发送到 API
3. 第三步: 处理响应
4. 第四步: 渲染结果

无序列表:
- 快速响应
- 安全可靠
- 易于集成

## 引用

> 这是一个引用块。
> 可以用来显示重要信息或来自其他来源的内容。

## 强调

**粗体文本** 和 *斜体文本* 和 ~~删除线~~ 和 \`行内代码\`

---

*这是测试结束*`;

interface StyleConfig {
  fontSize: string;
  lineHeight: string;
  codeFontSize: string;
  heading1Size: string;
  heading2Size: string;
  heading3Size: string;
  paragraphSpacing: string;
  listSpacing: string;
  blockquotePadding: string;
  blockquoteBorderLeft: string;
  codeBlockPadding: string;
  codeBlockBorderRadius: string;
  tableCellPadding: string;
  linkColor: string;
  linkDecoration: string;
  codeBackground: string;
  codeColor: string;
}

const DEFAULT_STYLES: StyleConfig = {
  fontSize: "12px",
  lineHeight: "1.2",
  codeFontSize: "11px",
  heading1Size: "20px",
  heading2Size: "16px",
  heading3Size: "14px",
  paragraphSpacing: "8px",
  listSpacing: "4px",
  blockquotePadding: "12px 8px",
  blockquoteBorderLeft: "4px solid #6366f1",
  codeBlockPadding: "8px",
  codeBlockBorderRadius: "0px",
  tableCellPadding: "4px",
  linkColor: "#6366f1",
  linkDecoration: "underline",
  codeBackground: "#f1f5f9",
  codeColor: "#334155",
};

function ChatView({
  styleConfig,
}: {
  styleConfig: StyleConfig;
}) {
  return (
    <div
      className="flex-1 overflow-y-auto p-4"
      style={{
        "--sd-font-size": styleConfig.fontSize,
        "--sd-line-height": styleConfig.lineHeight,
        "--sd-heading-1-size": styleConfig.heading1Size,
        "--sd-heading-2-size": styleConfig.heading2Size,
        "--sd-heading-3-size": styleConfig.heading3Size,
        "--sd-paragraph-spacing": styleConfig.paragraphSpacing,
        "--sd-list-spacing": styleConfig.listSpacing,
        "--sd-blockquote-padding": styleConfig.blockquotePadding,
        "--sd-blockquote-border": styleConfig.blockquoteBorderLeft,
        "--sd-code-font-size": styleConfig.codeFontSize,
        "--sd-code-background": styleConfig.codeBackground,
        "--sd-code-color": styleConfig.codeColor,
        "--sd-code-block-padding": styleConfig.codeBlockPadding,
        "--sd-code-block-radius": styleConfig.codeBlockBorderRadius,
        "--sd-table-cell-padding": styleConfig.tableCellPadding,
        "--sd-link-color": styleConfig.linkColor,
        "--sd-link-decoration": styleConfig.linkDecoration,
      } as React.CSSProperties}
    >
      <div className="max-w-4xl mx-auto">
        {/* 用户消息 - 右侧显示，带背景 */}
        <div className="flex justify-end mb-4">
          <div className="bg-slate-100 rounded-2xl px-4 py-3 max-w-[85%]">
            <p className="text-sm">你好，请介绍一下 ArgusClaw 的功能</p>
          </div>
        </div>

        {/* AI 消息 - 主体内容，无边框 */}
        <div className="mb-4">
          <div className="p-4 streamdown-content">
            <Streamdown
            mode="static"
            plugins={{ math, cjk }}
            components={{
              pre: ({ children }) => {
                // Extract code element from pre
                const codeEl = children as React.ReactElement<{ className?: string; children?: string }>;
                const className = codeEl?.props?.className || "";
                const langMatch = className.match(/language-(\w+)/);
                const language = langMatch ? langMatch[1] : "";
                const code = codeEl?.props?.children || "";

                return (
                  <CodeBlock
                    className={className}
                    language={language}
                    code={typeof code === "string" ? code : ""}
                  >
                    {code}
                  </CodeBlock>
                );
              },
            }}
          >
            {SAMPLE_MARKDOWN}
          </Streamdown>
          </div>
        </div>
      </div>
    </div>
  );
}

function StyleControls({
  config,
  onChange,
}: {
  config: StyleConfig;
  onChange: (config: StyleConfig) => void;
}) {
  const [activeTab, setActiveTab] = useState<
    "text" | "headings" | "code" | "blocks"
  >("text");

  const updateField = (field: keyof StyleConfig, value: string) => {
    onChange({ ...config, [field]: value });
  };

  const resetToDefault = () => {
    onChange({ ...DEFAULT_STYLES });
  };

  const tabs = [
    { id: "text", label: "文本" },
    { id: "headings", label: "标题" },
    { id: "code", label: "代码" },
    { id: "blocks", label: "块" },
  ] as const;

  return (
    <div className="w-72 border-l bg-white flex flex-col h-full overflow-y-auto">
      <div className="p-4 border-b">
        <h3 className="font-semibold text-sm">样式配置</h3>
        <p className="text-xs text-muted-foreground mt-1">
          调整 Streamdown 渲染样式
        </p>
      </div>

      {/* Tabs */}
      <div className="flex border-b">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`flex-1 py-2 text-xs font-medium transition-colors ${
              activeTab === tab.id
                ? "text-indigo-600 border-b-2 border-indigo-600 bg-indigo-50"
                : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Controls */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {activeTab === "text" && (
          <>
            <div className="space-y-2">
              <label className="text-xs font-medium">字体大小</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="12"
                  max="24"
                  value={parseInt(config.fontSize)}
                  onChange={(e) =>
                    updateField("fontSize", `${e.target.value}px`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">{config.fontSize}</span>
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">行高</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="120"
                  max="220"
                  value={parseInt(config.lineHeight) * 100}
                  onChange={(e) =>
                    updateField("lineHeight", `${Number(e.target.value) / 100}`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">{config.lineHeight}</span>
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">段落间距</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="8"
                  max="32"
                  value={parseInt(config.paragraphSpacing)}
                  onChange={(e) =>
                    updateField("paragraphSpacing", `${e.target.value}px`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">{config.paragraphSpacing}</span>
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">链接颜色</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="color"
                  value={config.linkColor}
                  onChange={(e) => updateField("linkColor", e.target.value)}
                  className="w-8 h-8 p-0 border-0"
                />
                <Input
                  value={config.linkColor}
                  onChange={(e) => updateField("linkColor", e.target.value)}
                  className="flex-1 text-xs font-mono"
                />
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">链接装饰</label>
              <select
                value={config.linkDecoration}
                onChange={(e) =>
                  updateField("linkDecoration", e.target.value)
                }
                className="w-full text-xs border rounded-md px-2 py-1.5"
              >
                <option value="underline">下划线</option>
                <option value="none">无</option>
                <option value="overline">上划线</option>
                <option value="line-through">删除线</option>
              </select>
            </div>
          </>
        )}

        {activeTab === "headings" && (
          <>
            <div className="space-y-2">
              <label className="text-xs font-medium">H1 大小</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="20"
                  max="40"
                  value={parseInt(config.heading1Size)}
                  onChange={(e) =>
                    updateField("heading1Size", `${e.target.value}px`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">{config.heading1Size}</span>
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">H2 大小</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="16"
                  max="32"
                  value={parseInt(config.heading2Size)}
                  onChange={(e) =>
                    updateField("heading2Size", `${e.target.value}px`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">{config.heading2Size}</span>
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">H3 大小</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="14"
                  max="26"
                  value={parseInt(config.heading3Size)}
                  onChange={(e) =>
                    updateField("heading3Size", `${e.target.value}px`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">{config.heading3Size}</span>
              </div>
            </div>
          </>
        )}

        {activeTab === "code" && (
          <>
            <div className="space-y-2">
              <label className="text-xs font-medium">代码字体大小</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="11"
                  max="18"
                  value={parseInt(config.codeFontSize)}
                  onChange={(e) =>
                    updateField("codeFontSize", `${e.target.value}px`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">{config.codeFontSize}</span>
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">代码块背景</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="color"
                  value={config.codeBackground}
                  onChange={(e) =>
                    updateField("codeBackground", e.target.value)
                  }
                  className="w-8 h-8 p-0 border-0"
                />
                <Input
                  value={config.codeBackground}
                  onChange={(e) =>
                    updateField("codeBackground", e.target.value)
                  }
                  className="flex-1 text-xs font-mono"
                />
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">行内代码颜色</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="color"
                  value={config.codeColor}
                  onChange={(e) => updateField("codeColor", e.target.value)}
                  className="w-8 h-8 p-0 border-0"
                />
                <Input
                  value={config.codeColor}
                  onChange={(e) => updateField("codeColor", e.target.value)}
                  className="flex-1 text-xs font-mono"
                />
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">代码块圆角</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="0"
                  max="16"
                  value={parseInt(config.codeBlockBorderRadius)}
                  onChange={(e) =>
                    updateField("codeBlockBorderRadius", `${e.target.value}px`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">
                  {config.codeBlockBorderRadius}
                </span>
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">代码块内边距</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="8"
                  max="32"
                  value={parseInt(config.codeBlockPadding)}
                  onChange={(e) =>
                    updateField("codeBlockPadding", `${e.target.value}px`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">{config.codeBlockPadding}</span>
              </div>
            </div>
          </>
        )}

        {activeTab === "blocks" && (
          <>
            <div className="space-y-2">
              <label className="text-xs font-medium">引用块左边框</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="color"
                  value={config.blockquoteBorderLeft.split(" ")[2]}
                  onChange={(e) =>
                    updateField(
                      "blockquoteBorderLeft",
                      `4px solid ${e.target.value}`
                    )
                  }
                  className="w-8 h-8 p-0 border-0"
                />
                <Input
                  value={config.blockquoteBorderLeft}
                  onChange={(e) =>
                    updateField("blockquoteBorderLeft", e.target.value)
                  }
                  className="flex-1 text-xs font-mono"
                />
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">引用块内边距</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="8"
                  max="24"
                  value={parseInt(config.blockquotePadding.split(" ")[1])}
                  onChange={(e) =>
                    updateField("blockquotePadding", `12px ${e.target.value}px`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">
                  {config.blockquotePadding}
                </span>
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">表格单元格内边距</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="4"
                  max="20"
                  value={parseInt(config.tableCellPadding)}
                  onChange={(e) =>
                    updateField("tableCellPadding", `${e.target.value}px`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">{config.tableCellPadding}</span>
              </div>
            </div>
            <div className="space-y-2">
              <label className="text-xs font-medium">列表间距</label>
              <div className="flex gap-2 items-center">
                <Input
                  type="range"
                  min="4"
                  max="24"
                  value={parseInt(config.listSpacing)}
                  onChange={(e) =>
                    updateField("listSpacing", `${e.target.value}px`)
                  }
                  className="flex-1"
                />
                <span className="text-xs w-12">{config.listSpacing}</span>
              </div>
            </div>
          </>
        )}
      </div>

      {/* Reset Button */}
      <div className="p-4 border-t">
        <Button variant="outline" onClick={resetToDefault} className="w-full">
          重置为默认
        </Button>
      </div>
    </div>
  );
}

function App() {
  const [currentPage, setCurrentPage] = useState<Page>("chat");
  const [styleConfig, setStyleConfig] = useState<StyleConfig>(DEFAULT_STYLES);

  return (
    <TooltipProvider>
      <SidebarProvider defaultOpen={true}>
        <Sidebar variant="floating" collapsible="offcanvas">
          <SidebarHeader className="py-4">
            <div className="flex items-center justify-center">
              <span className="text-lg font-semibold">ArgusClaw</span>
            </div>
          </SidebarHeader>
          <SidebarContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton
                  isActive={currentPage === "chat"}
                  onClick={() => setCurrentPage("chat")}
                >
                  <HugeiconsIcon icon={ChatIcon} />
                  <span>聊天</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton
                  isActive={currentPage === "streamdown-dev"}
                  onClick={() => setCurrentPage("streamdown-dev")}
                >
                  <HugeiconsIcon icon={CodeIcon} />
                  <span>Streamdown 开发</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarContent>
        </Sidebar>
        <SidebarInset>
          <header className="flex h-14 items-center gap-2 border-b px-4">
            <SidebarTrigger />
            <span className="text-sm font-medium">
              {currentPage === "chat" ? "聊天" : "Streamdown 开发"}
            </span>
          </header>
          {currentPage === "chat" ? (
            <ChatPage />
          ) : (
            <StreamdownDevPage
              styleConfig={styleConfig}
              onStyleChange={setStyleConfig}
            />
          )}
        </SidebarInset>
      </SidebarProvider>
    </TooltipProvider>
  );
}

// 聊天页面 - 占位页面
function ChatPage() {
  return (
    <div className="flex-1 flex items-center justify-center text-muted-foreground">
      <div className="text-center">
        <HugeiconsIcon icon={AiMagicIcon} className="w-12 h-12 mx-auto mb-4 opacity-50" />
        <p>选择一个对话开始</p>
      </div>
    </div>
  );
}

// Streamdown 开发页面 - 带样式调试
function StreamdownDevPage({
  styleConfig,
  onStyleChange,
}: {
  styleConfig: StyleConfig;
  onStyleChange: (config: StyleConfig) => void;
}) {
  return (
    <div className="flex h-full">
      <ChatView styleConfig={styleConfig} />
      <StyleControls config={styleConfig} onChange={onStyleChange} />
    </div>
  );
}

export default App;
