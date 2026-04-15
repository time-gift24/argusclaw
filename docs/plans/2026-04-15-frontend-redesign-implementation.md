# Frontend Redesign Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 按照 DESIGN.md 的暖色纸张风格完成前端重设计，实现沉浸式聊天界面和 Notion 式设置导航。

**Architecture:** 以 assistant-ui primitives 为基础重写 UI 层，globals.css 管理 CSS 变量和 DESIGN.md 色彩系统，保留原有 runtime 逻辑不动。

**Tech Stack:** React 19, Vite, Tailwind CSS 4, @assistant-ui/react, Tauri 2

---

## Phase 1: CSS Foundation

### Task 1: Update globals.css with DESIGN.md Color System

**Files:**
- Modify: `crates/desktop/app/globals.css`

**Step 1: Write the failing test (implicit)**

Verify Tailwind loads and CSS variables are applied:
```css
/* After changes, verify these tokens exist */
--color-parchment: #f5f4ed;
--color-ivory: #faf9f5;
--color-terracotta: #c96442;
```

**Step 2: Backup existing globals.css**

Run: `cp crates/desktop/app/globals.css crates/desktop/app/globals.css.bak`

**Step 3: Rewrite globals.css**

Replace the entire file content with:

```css
@import "tailwindcss";
@import "tw-animate-css";
@import "shadcn/tailwind.css";

@custom-variant dark (&:is(.dark *));

/* DESIGN.md Color System - Warm Parchment Palette */
:root {
  /* Brand Colors */
  --color-terracotta: #c96442;
  --color-coral: #d97757;

  /* Surface & Background */
  --color-parchment: #f5f4ed;
  --color-ivory: #faf9f5;
  --color-white: #ffffff;
  --color-sand: #e8e6dc;
  --color-dark-surface: #30302e;
  --color-near-black: #141413;

  /* Text Colors */
  --color-charcoal: #4d4c48;
  --color-olive: #5e5d59;
  --color-stone: #87867f;
  --color-dark-warm: #3d3d3a;
  --color-warm-silver: #b0aea5;

  /* Borders */
  --color-border-cream: #f0eee6;
  --color-border-warm: #e8e6dc;

  /* Semantic */
  --color-error: #b53333;
  --color-focus-blue: #3898ec;

  /* Ring Shadows */
  --color-ring-warm: #d1cfc5;
  --color-ring-subtle: #dedc01;
  --color-ring-deep: #c2c0b6;

  /* Map to Tailwind / shadcn tokens */
  --background: var(--color-parchment);
  --foreground: var(--color-near-black);
  --card: var(--color-ivory);
  --card-foreground: var(--color-near-black);
  --popover: var(--color-ivory);
  --popover-foreground: var(--color-near-black);
  --primary: var(--color-terracotta);
  --primary-foreground: var(--color-ivory);
  --secondary: var(--color-sand);
  --secondary-foreground: var(--color-charcoal);
  --muted: var(--color-sand);
  --muted-foreground: var(--color-olive);
  --accent: var(--color-sand);
  --accent-foreground: var(--color-charcoal);
  --destructive: var(--color-error);
  --border: var(--color-border-cream);
  --input: var(--color-border-warm);
  --ring: var(--color-ring-warm);

  /* Font Families */
  --font-sans-base: "Segoe UI", "Microsoft YaHei", "Inter", "PingFang SC", "Hiragino Sans GB", "Noto Sans SC", system-ui, sans-serif;
  --font-mono-base: "SF Mono", "SFMono-Regular", "JetBrains Mono", "Menlo", "Consolas", monospace;
  --font-serif-base: "Georgia", "Times New Roman", serif;

  /* Radius */
  --radius: 0.5rem;
}

.dark {
  /* Keep minimal dark support for future */
  --background: var(--color-near-black);
  --foreground: var(--color-ivory);
  --card: var(--color-dark-surface);
  --card-foreground: var(--color-ivory);
  --primary: var(--color-terracotta);
  --primary-foreground: var(--color-ivory);
  --secondary: var(--color-dark-surface);
  --muted: var(--color-dark-surface);
  --muted-foreground: var(--color-stone);
  --border: var(--color-dark-surface);
  --ring: var(--color-ring-warm);
}

@theme inline {
  --font-sans: var(--font-sans-base);
  --font-mono: var(--font-mono-base);
  --font-serif: var(--font-serif-base);
  --color-primary: var(--primary);
  --color-primary-foreground: var(--primary-foreground);
  --color-secondary: var(--secondary);
  --color-secondary-foreground: var(--secondary-foreground);
  --color-muted: var(--muted);
  --color-muted-foreground: var(--muted-foreground);
  --color-accent: var(--accent);
  --color-accent-foreground: var(--accent-foreground);
  --color-destructive: var(--destructive);
  --color-border: var(--border);
  --color-input: var(--input);
  --color-ring: var(--ring);
  --color-background: var(--background);
  --color-foreground: var(--foreground);
  --color-card: var(--card);
  --color-card-foreground: var(--card-foreground);
  --color-parchment: var(--color-parchment);
  --color-ivory: var(--color-ivory);
  --color-terracotta: var(--color-terracotta);
  --color-coral: var(--color-coral);
  --color-charcoal: var(--color-charcoal);
  --color-olive: var(--color-olive);
  --color-stone: var(--color-stone);
  --color-sand: var(--color-sand);
  --color-near-black: var(--color-near-black);
  --color-dark-surface: var(--color-dark-surface);
  --color-warm-silver: var(--color-warm-silver);
  --color-border-cream: var(--color-border-cream);
  --color-border-warm: var(--color-border-warm);
  --color-ring-warm: var(--color-ring-warm);
  --radius-sm: calc(var(--radius) * 0.6);
  --radius-md: calc(var(--radius) * 0.8);
  --radius-lg: var(--radius);
  --radius-xl: calc(var(--radius) * 1.5);
  --radius-2xl: calc(var(--radius) * 2);
  --radius-3xl: calc(var(--radius) * 3);
}

@layer base {
  * {
    @apply border-border outline-ring/50;
  }
  html,
  body {
    height: 100%;
    @apply scroll-smooth;
  }
  html {
    @apply font-sans text-sm sm:text-base;
  }
  body {
    @apply bg-background text-foreground antialiased;
  }

  /* Serif headings */
  h1, h2, h3 {
    font-family: var(--font-serif-base);
    font-weight: 500;
  }

  /* Code styling */
  code, pre {
    font-family: var(--font-mono-base);
  }
}
```

**Step 4: Verify build works**

Run: `cd crates/desktop && pnpm build`
Expected: No CSS errors

**Step 5: Commit**

```bash
git add crates/desktop/app/globals.css
git commit -m "feat(ui): implement DESIGN.md color system in globals.css

- Add warm parchment palette (parchment, ivory, terracotta, olive neutrals)
- Map CSS variables to Tailwind/shadcn tokens
- Add serif font family for headings
- Keep minimal dark theme placeholder for future

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2: Create Typography Utility Classes

**Files:**
- Modify: `crates/desktop/app/globals.css`

**Step 1: Add typography utilities to globals.css**

Add before `@layer base`:

```css
/* Typography Utilities - DESIGN.md inspired */
@layer utilities {
  .font-serif {
    font-family: var(--font-serif-base);
  }

  .text-display {
    font-family: var(--font-serif-base);
    font-size: 3rem;
    font-weight: 500;
    line-height: 1.10;
  }

  .text-heading-lg {
    font-family: var(--font-serif-base);
    font-size: 2.25rem;
    font-weight: 500;
    line-height: 1.20;
  }

  .text-heading-md {
    font-family: var(--font-serif-base);
    font-size: 1.75rem;
    font-weight: 500;
    line-height: 1.25;
  }

  .text-heading-sm {
    font-family: var(--font-serif-base);
    font-size: 1.5rem;
    font-weight: 500;
    line-height: 1.30;
  }

  .text-body-lg {
    font-size: 1.25rem;
    line-height: 1.60;
  }

  .text-body {
    font-size: 1rem;
    line-height: 1.50;
  }

  .text-caption {
    font-size: 0.875rem;
    line-height: 1.43;
  }

  .text-overline {
    font-size: 0.625rem;
    font-weight: 500;
    letter-spacing: 0.5px;
    text-transform: uppercase;
  }

  .leading-relaxed {
    line-height: 1.60;
  }

  .leading-snug {
    line-height: 1.30;
  }
}
```

**Step 2: Verify build**

Run: `cd crates/desktop && pnpm build`

**Step 3: Commit**

```bash
git add crates/desktop/app/globals.css
git commit -m "feat(ui): add typography utility classes

- Serif headings (display, heading-lg/md/sm)
- Body text with relaxed line-height (1.60)
- Caption and overline styles
- Follows DESIGN.md typographic hierarchy

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Phase 2: Base Component Library

### Task 3: Create Button Component

**Files:**
- Modify: `crates/desktop/components/ui/button.tsx`

**Step 1: Read existing button.tsx**

Run: `cat crates/desktop/components/ui/button.tsx`

**Step 2: Rewrite with DESIGN.md styles**

```tsx
import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-lg text-sm font-medium transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring-warm focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        // Primary CTA - Terracotta brand
        default:
          "bg-terracotta text-ivory shadow-md hover:bg-terracotta/90 active:scale-[0.98]",
        // Secondary - Sand background
        secondary:
          "bg-sand text-charcoal border border-border-warm shadow-sm hover:bg-sand/80 active:scale-[0.98]",
        // Ghost - Minimal
        ghost:
          "hover:bg-sand/50 text-charcoal",
        // Outline
        outline:
          "border border-border-warm bg-transparent hover:bg-sand/30 text-charcoal",
        // Dark surface for dark contexts
        dark:
          "bg-dark-surface text-ivory border border-dark-surface hover:bg-dark-surface/80",
      },
      size: {
        default: "h-10 px-4 py-2",
        sm: "h-8 px-3 text-xs",
        lg: "h-12 px-6 text-base",
        icon: "h-10 w-10",
        "icon-sm": "h-8 w-8",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
)

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, ...props }, ref) => {
    return (
      <button
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        {...props}
      />
    )
  }
)
Button.displayName = "Button"

export { Button, buttonVariants }
```

**Step 3: Commit**

```bash
git add crates/desktop/components/ui/button.tsx
git commit -m "feat(ui): rewrite Button with DESIGN.md styles

- Terracotta primary CTA variant
- Sand secondary variant
- Warm ring shadows
- Soft rounded corners (8px)
- Follows warm neutral palette

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 4: Create Card Component

**Files:**
- Modify: `crates/desktop/components/ui/card.tsx`

**Step 1: Read existing card.tsx**

Run: `cat crates/desktop/components/ui/card.tsx`

**Step 2: Rewrite with DESIGN.md styles**

```tsx
import * as React from "react"
import { cn } from "@/lib/utils"

const Card = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, ...props }, ref) => (
  <div
    ref={ref}
    className={cn(
      "rounded-xl border border-border-cream bg-ivory text-card-foreground shadow-sm",
      className
    )}
    {...props}
  />
))
Card.displayName = "Card"

const CardHeader = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, ...props }, ref) => (
  <div
    ref={ref}
    className={cn("flex flex-col space-y-1.5 p-6", className)}
    {...props}
  />
))
CardHeader.displayName = "CardHeader"

const CardTitle = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLHeadingElement>
>(({ className, ...props }, ref) => (
  <h3
    ref={ref}
    className={cn(
      "font-serif text-xl font-medium leading-snug text-near-black",
      className
    )}
    {...props}
  />
))
CardTitle.displayName = "CardTitle"

const CardDescription = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <p
    ref={ref}
    className={cn("text-sm text-olive leading-relaxed", className)}
    {...props}
  />
))
CardDescription.displayName = "CardDescription"

const CardContent = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, ...props }, ref) => (
  <div ref={ref} className={cn("p-6 pt-0", className)} {...props} />
))
CardContent.displayName = "CardContent"

const CardFooter = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, ...props }, ref) => (
  <div
    ref={ref}
    className={cn("flex items-center p-6 pt-0", className)}
    {...props}
  />
))
CardFooter.displayName = "CardFooter"

export { Card, CardHeader, CardFooter, CardTitle, CardDescription, CardContent }
```

**Step 3: Commit**

```bash
git add crates/desktop/components/ui/card.tsx
git commit -m "feat(ui): rewrite Card with DESIGN.md styles

- Ivory background with border-cream border
- Serif titles with tight line-height
- Warm text colors (olive, charcoal)
- Soft shadow-sm

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 5: Create Input Component

**Files:**
- Modify: `crates/desktop/components/ui/input.tsx`

**Step 1: Read existing input.tsx**

Run: `cat crates/desktop/components/ui/input.tsx`

**Step 2: Rewrite with DESIGN.md styles**

```tsx
import * as React from "react"
import { cn } from "@/lib/utils"

export interface InputProps
  extends React.InputHTMLAttributes<HTMLInputElement> {}

const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        type={type}
        className={cn(
          "flex h-10 w-full rounded-lg border border-border-warm bg-sand/50 px-4 py-2 text-sm text-near-black",
          "placeholder:text-stone",
          "transition-all duration-200",
          "focus:outline-none focus:ring-2 focus:ring-terracotta/30 focus:border-terracotta",
          "disabled:cursor-not-allowed disabled:opacity-50",
          className
        )}
        ref={ref}
        {...props}
      />
    )
  }
)
Input.displayName = "Input"

export { Input }
```

**Step 3: Commit**

```bash
git add crates/desktop/components/ui/input.tsx
git commit -m "feat(ui): rewrite Input with DESIGN.md styles

- Sand background with warm border
- Terracotta focus ring
- Charcoal text, stone placeholder

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Phase 3: Layout Components

### Task 6: Create MinimalTopBar Component

**Files:**
- Create: `crates/desktop/components/layout/minimal-top-bar.tsx`

**Step 1: Create directory and file**

```tsx
"use client"

import { useAuthStore } from "@/components/auth/use-auth-store"
import { Button } from "@/components/ui/button"
import { Settings, Bot } from "lucide-react"
import { useState } from "react"
import { SettingsDrawer } from "./settings-drawer"

export function MinimalTopBar() {
  const fetchCurrentUser = useAuthStore((s) => s.fetchCurrentUser)
  const [settingsOpen, setSettingsOpen] = useState(false)

  return (
    <>
      <header className="fixed top-0 left-0 right-0 z-50 h-14 border-b border-border-cream bg-parchment/95 backdrop-blur-sm">
        <div className="flex h-full items-center justify-between px-4">
          {/* Left: Logo */}
          <div className="flex items-center gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-terracotta text-ivory">
              <Bot className="h-5 w-5" />
            </div>
            <span className="font-serif text-lg font-medium text-near-black">
              ArgusWing
            </span>
          </div>

          {/* Right: Actions */}
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setSettingsOpen(true)}
              aria-label="打开设置"
            >
              <Settings className="h-5 w-5 text-charcoal" />
            </Button>
          </div>
        </div>
      </header>

      <SettingsDrawer open={settingsOpen} onOpenChange={setSettingsOpen} />
    </>
  )
}
```

**Step 2: Commit**

```bash
git add crates/desktop/components/layout/minimal-top-bar.tsx
git commit -m "feat(ui): create MinimalTopBar component

- Fixed top bar, 56px height
- Logo with Terracotta icon
- Settings gear icon button
- Follows minimal chrome philosophy

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 7: Create SettingsDrawer Component

**Files:**
- Create: `crates/desktop/components/layout/settings-drawer.tsx`

**Step 1: Create SettingsDrawer with Notion-style sidebar**

```tsx
"use client"

import { X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { useNavigate, useLocation } from "react-router-dom"

interface SettingsDrawerProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

const settingsNavItems = [
  {
    label: "Agents",
    href: "/settings/agents",
    description: "管理 Agent 配置",
  },
  {
    label: "Providers",
    href: "/settings/providers",
    description: "配置 LLM Providers",
  },
  {
    label: "Tools",
    href: "/settings/tools",
    description: "内置工具管理",
  },
  {
    label: "MCP",
    href: "/settings/mcp",
    description: "MCP Server 配置",
  },
]

export function SettingsDrawer({ open, onOpenChange }: SettingsDrawerProps) {
  const navigate = useNavigate()
  const location = useLocation()

  const handleNav = (href: string) => {
    navigate(href)
    onOpenChange(false)
  }

  if (!open) return null

  return (
    <>
      {/* Backdrop */}
      <div
        className="fixed inset-0 z-50 bg-near-black/20 backdrop-blur-sm"
        onClick={() => onOpenChange(false)}
      />

      {/* Drawer */}
      <div className="fixed inset-y-0 right-0 z-50 flex">
        <div className="w-72 border-l border-border-cream bg-parchment shadow-xl">
          {/* Header */}
          <div className="flex h-14 items-center justify-between border-b border-border-cream px-4">
            <span className="font-serif text-lg font-medium text-near-black">
              设置
            </span>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => onOpenChange(false)}
            >
              <X className="h-5 w-5" />
            </Button>
          </div>

          {/* Navigation */}
          <nav className="p-3">
            <ul className="space-y-1">
              {settingsNavItems.map((item) => {
                const isActive = location.pathname.startsWith(item.href)
                return (
                  <li key={item.href}>
                    <button
                      onClick={() => handleNav(item.href)}
                      className={cn(
                        "flex w-full flex-col items-start gap-0.5 rounded-lg px-3 py-2.5 text-left transition-colors",
                        isActive
                          ? "bg-sand border-l-2 border-terracotta font-medium text-near-black"
                          : "hover:bg-sand/50 text-charcoal"
                      )}
                    >
                      <span className="text-sm">{item.label}</span>
                      <span className="text-xs text-stone">{item.description}</span>
                    </button>
                  </li>
                )
              })}
            </ul>
          </nav>
        </div>
      </div>
    </>
  )
}
```

**Step 2: Commit**

```bash
git add crates/desktop/components/layout/settings-drawer.tsx
git commit -m "feat(ui): create SettingsDrawer with Notion-style nav

- Slide-in drawer from right
- Notion-style sidebar navigation
- Terracotta active indicator
- Warm parchment background

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 8: Create Layout Directory Index

**Files:**
- Create: `crates/desktop/components/layout/index.ts`

**Step 1: Create index export**

```tsx
export { MinimalTopBar } from "./minimal-top-bar"
export { SettingsDrawer } from "./settings-drawer"
```

**Step 2: Commit**

```bash
git add crates/desktop/components/layout/index.ts
git commit -m "chore(ui): export layout components

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Phase 4: Chat UI Rewrite

### Task 9: Create ThreadWelcome Component

**Files:**
- Modify: `crates/desktop/components/assistant-ui/thread.tsx` (create backup first)

**Step 1: Backup existing thread.tsx**

Run: `cp crates/desktop/components/assistant-ui/thread.tsx crates/desktop/components/assistant-ui/thread.tsx.bak`

**Step 2: Create ThreadWelcome component**

Add to thread.tsx:

```tsx
const ThreadWelcome: FC = () => {
  return (
    <div className="aui-thread-welcome-root mx-auto my-auto flex w-full max-w-2xl grow flex-col">
      <div className="aui-thread-welcome-center flex w-full grow flex-col items-center justify-center py-16 px-4">
        {/* Bot Icon */}
        <div className="mb-8 rounded-[2rem] bg-sand/80 p-5 text-terracotta shadow-md">
          <Bot className="size-12" />
        </div>

        {/* Welcome Message */}
        <h1 className="font-serif text-4xl font-medium text-near-black text-center mb-4 leading-snug">
          欢迎来到 ArgusWing
        </h1>
        <p className="text-body-lg text-olive text-center max-w-md leading-relaxed">
          我是您的 AI 助手，今天有什么可以帮您的？
        </p>
      </div>

      {/* Quick Start */}
      <div className="px-4 pb-12">
        <div className="mb-4 flex items-center gap-2 px-1">
          <Sparkles className="h-3 w-3 text-terracotta" />
          <span className="text-overline text-stone">快速开始</span>
        </div>
        <ThreadSuggestions />
      </div>
    </div>
  )
}
```

**Step 3: Commit**

```bash
git add crates/desktop/components/assistant-ui/thread.tsx
git commit -m "feat(ui): add ThreadWelcome with DESIGN.md style

- Serif display heading (4xl)
- Centered layout with generous spacing
- Terracotta bot icon
- Warm parchment background

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 10: Rewrite Composer Component

**Files:**
- Modify: `crates/desktop/components/assistant-ui/thread.tsx`

**Step 1: Add redesigned Composer**

Replace the existing Composer and ComposerAction with:

```tsx
const ComposerAction: FC = () => {
  const session = useActiveChatSession();
  const isRunning = useAuiState((s) => s.thread.isRunning);
  const isCompacting = session?.status === "compacting";
  const aui = useAui();

  const handleCancel = () => {
    void useChatStore.getState().cancelTurn();
    try {
      aui.thread().cancelRun();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error ?? "");
      if (!message.includes("does not support cancelling runs")) {
        console.error("取消运行失败:", error);
      }
    }
  };

  return (
    <div className="flex items-center justify-between gap-3">
      <div className="flex items-center gap-2">
        <NewSessionButton />
        <SessionHistoryButton />
        <AgentSelector />
        <ProviderSelector />
      </div>
      <div className="flex items-center gap-3">
        {session && session.tokenCount > 0 && session.contextWindow && (
          <TokenRing
            modelContextWindow={session.contextWindow}
            tokenCount={session.tokenCount}
            className="size-8 opacity-80"
          />
        )}

        {isRunning ? (
          <Button
            variant="ghost"
            size="icon"
            className="size-9 rounded-full text-destructive hover:bg-destructive/10"
            onClick={handleCancel}
            aria-label="停止生成"
          >
            <StopCircle className="size-5" />
          </Button>
        ) : (
          <ComposerPrimitive.Send asChild>
            <Button
              size="icon"
              className="size-10 rounded-full bg-terracotta text-ivory shadow-lg shadow-terracotta/25 transition-all hover:bg-terracotta/90 active:scale-95 disabled:opacity-50"
              disabled={isCompacting}
              aria-label="发送消息"
            >
              <ArrowUpIcon className="size-5" />
            </Button>
          </ComposerPrimitive.Send>
        )}
      </div>
    </div>
  );
};

const Composer: FC = () => {
  const session = useActiveChatSession();
  const isCompacting = session?.status === "compacting";

  return (
    <ComposerPrimitive.Root className="relative flex w-full flex-col">
      <ComposerPrimitive.AttachmentDropzone
        className={cn(
          "flex flex-col rounded-3xl border bg-ivory shadow-lg shadow-terracotta/5",
          "transition-all duration-300",
          "has-[textarea:focus-visible]:border-terracotta/40 has-[textarea:focus-visible]:ring-2 has-[textarea:focus-visible]:ring-terracotta/20",
          "data-[dragging=true]:border-terracotta data-[dragging=true]:bg-terracotta/5"
        )}
      >
        <ComposerAttachments />
        <ComposerPrimitive.Input
          placeholder="给 ArgusWing 发送消息..."
          className="mb-2 max-h-48 min-h-14 w-full resize-none bg-transparent px-6 pt-4 pb-3 text-body text-near-black placeholder:text-stone/70 outline-none"
          rows={1}
          autoFocus
          disabled={isCompacting}
          aria-label="消息输入"
        />
        <div className="px-4 pb-4">
          <ComposerAction />
        </div>
      </ComposerPrimitive.AttachmentDropzone>
    </ComposerPrimitive.Root>
  );
};
```

**Step 2: Commit**

```bash
git add crates/desktop/components/assistant-ui/thread.tsx
git commit -m "feat(ui): rewrite Composer with DESIGN.md styles

- Terracotta send button with shadow
- Ivory card background with ring shadow
- 24px rounded corners
- Warm color palette

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 11: Rewrite Message Components

**Files:**
- Modify: `crates/desktop/components/assistant-ui/thread.tsx`

**Step 1: Rewrite AssistantMessage**

Replace the existing AssistantMessage with:

```tsx
const AssistantMessage: FC = () => {
  return (
    <MessagePrimitive.Root
      className="aui-assistant-message-root mx-auto w-full max-w-3xl px-4 py-8"
      data-role="assistant"
    >
      <div className="aui-assistant-message-content wrap-break-word text-body-lg leading-relaxed text-near-black selection:bg-terracotta/10">
        <AssistantTurnArtifacts />
        <MessagePrimitive.Content components={{ Text: MarkdownText }} />
        <MessageError />
      </div>

      <div className="aui-assistant-message-footer mt-6 flex min-h-6 items-center gap-4 border-t border-border-cream pt-4 opacity-60">
        <BranchPicker />
        <AssistantActionBar />
      </div>
    </MessagePrimitive.Root>
  );
};
```

**Step 2: Rewrite UserMessage**

Replace the existing UserMessage with:

```tsx
const UserMessage: FC = () => {
  return (
    <MessagePrimitive.Root
      className="aui-user-message-root mx-auto w-full max-w-3xl px-4 py-6"
      data-role="user"
    >
      <UserMessageAttachments />
      <div className="aui-user-message-content-wrapper relative">
        <div className="aui-user-message-content wrap-break-word rounded-2xl bg-sand/70 border border-border-warm px-5 py-3 text-charcoal shadow-sm">
          <MessagePrimitive.Parts />
        </div>
        <div className="aui-user-action-bar-wrapper absolute top-1/2 left-0 -translate-x-full -translate-y-1/2 pr-3 opacity-0 group-hover:opacity-100 transition-opacity">
          <UserActionBar />
        </div>
      </div>
      <BranchPicker className="mt-3 justify-end" />
    </MessagePrimitive.Root>
  );
};
```

**Step 3: Commit**

```bash
git add crates/desktop/components/assistant-ui/thread.tsx
git commit -m "feat(ui): rewrite message components with DESIGN.md styles

- Assistant: serif body, terracotta accents, generous padding
- User: sand background, charcoal text, subtle styling
- Warm color palette throughout

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 12: Update Thread Container

**Files:**
- Modify: `crates/desktop/components/assistant-ui/thread.tsx`

**Step 1: Update Thread component styles**

Find the Thread export and update its className:

```tsx
export const Thread: FC = () => {
  const session = useActiveChatSession();

  return (
    <ThreadPrimitive.Root
      className="aui-root aui-thread-root relative flex h-full min-h-0 w-full flex-1 flex-col overflow-hidden bg-parchment"
      style={{
        ["--thread-max-width" as string]: "48rem",
        ["--composer-max-width" as string]: "36rem",
      }}
    >
      <ThreadPrimitive.Viewport
        autoScroll
        className="aui-thread-viewport relative flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto px-4 pt-20 pb-32 scroll-smooth"
      >
        <AuiIf condition={(s) => s.thread.isEmpty}>
          <ThreadWelcome />
        </AuiIf>

        {session && <CompactionGroups />}

        <ThreadPrimitive.Messages
          components={{
            UserMessage,
            EditComposer,
            AssistantMessage,
          }}
        />

        <div className="pointer-events-none sticky bottom-28 z-40 mx-auto flex w-fit">
          <ThreadPrimitive.ScrollToBottom asChild>
            <button className="pointer-events-auto flex size-9 items-center justify-center rounded-full border border-border-warm bg-ivory text-charcoal shadow-md transition-all hover:bg-sand disabled:pointer-events-none disabled:opacity-0">
              <ArrowDownIcon className="size-4" />
            </button>
          </ThreadPrimitive.ScrollToBottom>
        </div>
      </ThreadPrimitive.Viewport>

      {/* Floating Composer */}
      <div className="z-50 pointer-events-none flex justify-center pb-8 pt-4">
        <div className="w-full max-w-(--composer-max-width) px-4 pointer-events-auto">
          <JobStatusArtifacts />
          <PendingAssistantArtifacts />
          <ChatStatusBanner />
          <Composer />
        </div>
      </div>
      <SubagentJobDetailsDrawer />
    </ThreadPrimitive.Root>
  );
};
```

**Step 2: Commit**

```bash
git add crates/desktop/components/assistant-ui/thread.tsx
git commit -m "feat(ui): update Thread container styles

- Parchment background
- Max-width 48rem for messages
- Floating composer with pointer-events handling
- Generous padding (pt-20, pb-32)

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Phase 5: Settings Page Rewrite

### Task 13: Create SettingsSidebar Component

**Files:**
- Create: `crates/desktop/components/layout/settings-sidebar.tsx`

**Step 1: Create SettingsSidebar**

```tsx
"use client"

import { cn } from "@/lib/utils"
import { useNavigate, useLocation } from "react-router-dom"
import { Bot, Cpu, Wrench, Plug } from "lucide-react"

interface NavItem {
  label: string
  href: string
  icon: React.ReactNode
}

const navItems: NavItem[] = [
  { label: "Agents", href: "/settings/agents", icon: <Bot className="h-4 w-4" /> },
  { label: "Providers", href: "/settings/providers", icon: <Cpu className="h-4 w-4" /> },
  { label: "Tools", href: "/settings/tools", icon: <Wrench className="h-4 w-4" /> },
  { label: "MCP", href: "/settings/mcp", icon: <Plug className="h-4 w-4" /> },
]

export function SettingsSidebar() {
  const navigate = useNavigate()
  const location = useLocation()

  return (
    <aside className="fixed left-0 top-14 bottom-0 w-60 border-r border-border-cream bg-ivory overflow-y-auto">
      <div className="p-4">
        {/* Breadcrumb hint */}
        <div className="mb-4 text-overline text-stone">设置</div>

        <nav className="space-y-1">
          {navItems.map((item) => {
            const isActive = location.pathname.startsWith(item.href)
            return (
              <button
                key={item.href}
                onClick={() => navigate(item.href)}
                className={cn(
                  "flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-sm transition-all duration-150",
                  isActive
                    ? "bg-sand font-medium text-near-black border-l-2 border-terracotta"
                    : "text-charcoal hover:bg-sand/50"
                )}
              >
                <span className={cn(isActive ? "text-terracotta" : "text-stone")}>
                  {item.icon}
                </span>
                {item.label}
              </button>
            )
          })}
        </nav>
      </div>
    </aside>
  )
}
```

**Step 2: Commit**

```bash
git add crates/desktop/components/layout/settings-sidebar.tsx
git commit -m "feat(ui): create SettingsSidebar component

- Fixed left sidebar, 240px width
- Notion-style navigation
- Terracotta active indicator
- Ivory background

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 14: Update Settings Layout

**Files:**
- Modify: `crates/desktop/app/settings/layout.tsx`

**Step 1: Rewrite settings layout**

```tsx
"use client"

import { SettingsSidebar } from "@/components/layout/settings-sidebar"

export default function SettingsLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <div className="flex min-h-0 flex-1">
      <SettingsSidebar />
      <main className="flex-1 overflow-y-auto pl-60">
        <div className="mx-auto max-w-4xl px-8 py-8">
          {children}
        </div>
      </main>
    </div>
  )
}
```

**Step 2: Commit**

```bash
git add crates/desktop/app/settings/layout.tsx
git commit -m "feat(ui): update settings layout with sidebar

- SettingsSidebar fixed left
- Content area with max-w-4xl
- Generous padding

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 15: Update Root Layout

**Files:**
- Modify: `crates/desktop/app/layout.tsx`

**Step 1: Rewrite root layout**

```tsx
"use client"

import { useEffect } from "react"
import { ThemeProvider } from "@/components/theme-provider"
import { TooltipProvider } from "@/components/ui/tooltip"
import { ToastProvider } from "@/components/ui/toast"
import { MinimalTopBar } from "@/components/layout/minimal-top-bar"
import { useAuthStore } from "@/components/auth/use-auth-store"
import { LoginToast, useLoginToastStore } from "@/components/auth/login-toast"

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  const fetchCurrentUser = useAuthStore((state) => state.fetchCurrentUser)
  const { toast, hideToast } = useLoginToastStore()

  useEffect(() => {
    void fetchCurrentUser()
  }, [fetchCurrentUser])

  return (
    <div className="flex h-dvh min-h-dvh flex-col overflow-hidden bg-parchment font-sans antialiased">
      <TooltipProvider>
        <ToastProvider>
          <ThemeProvider>
            <MinimalTopBar />
            <div className="flex-1 pt-14">
              {children}
            </div>
          </ThemeProvider>
        </ToastProvider>
      </TooltipProvider>
      {toast && (
        <LoginToast
          message={toast.message}
          type={toast.type}
          onClose={hideToast}
        />
      )}
    </div>
  )
}
```

**Step 2: Commit**

```bash
git add crates/desktop/app/layout.tsx
git commit -m "feat(ui): update root layout with MinimalTopBar

- MinimalTopBar fixed at top
- Parchment background
- Content starts below top bar

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Phase 6: Verification & Polish

### Task 16: Run Build Verification

**Step 1: Run build**

Run: `cd crates/desktop && pnpm build`
Expected: Successful build with no TypeScript errors

**Step 2: Run dev server**

Run: `cd crates/desktop && pnpm dev`

**Step 3: Verify design tokens**

Check browser devtools for CSS variables:
- `--color-parchment` should be `#f5f4ed`
- `--color-terracotta` should be `#c96442`
- `--color-ivory` should be `#faf9f5`

**Step 4: Commit build verification**

```bash
git add -A
git commit -m "chore: verify build passes after redesign

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 17: Run Existing Tests

**Step 1: Run tests**

Run: `cd crates/desktop && pnpm test`
Expected: All existing tests pass

**Step 2: If tests fail, diagnose and fix**

Common issues:
- CSS class names changed → update test selectors
- Component structure changed → update test queries

**Step 3: Commit test fixes if needed**

```bash
git add -A
git commit -m "test: fix UI tests after redesign

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Summary

**Files Modified/Created:**
- `crates/desktop/app/globals.css` - Full redesign of CSS variables and typography
- `crates/desktop/app/layout.tsx` - MinimalTopBar integration
- `crates/desktop/app/settings/layout.tsx` - SettingsSidebar integration
- `crates/desktop/components/ui/button.tsx` - Terracotta/Sand variants
- `crates/desktop/components/ui/card.tsx` - Warm palette
- `crates/desktop/components/ui/input.tsx` - Warm focus states
- `crates/desktop/components/assistant-ui/thread.tsx` - Complete UI rewrite
- `crates/desktop/components/layout/minimal-top-bar.tsx` - New
- `crates/desktop/components/layout/settings-drawer.tsx` - New
- `crates/desktop/components/layout/settings-sidebar.tsx` - New
- `crates/desktop/components/layout/index.ts` - New

**Order of Implementation:**
1. CSS Foundation (Tasks 1-2)
2. Base Components (Tasks 3-5)
3. Layout Components (Tasks 6-8)
4. Chat UI (Tasks 9-12)
5. Settings Page (Tasks 13-15)
6. Verification (Tasks 16-17)
