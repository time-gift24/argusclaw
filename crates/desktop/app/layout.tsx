"use client"

import { useEffect } from "react"
import { Home } from "lucide-react"

import "./globals.css"
import { ThemeProvider } from "@/components/theme-provider"
import { TooltipProvider } from "@/components/ui/tooltip"
import { ToastProvider } from "@/components/ui/toast"
import DashboardShell from "@/components/shadcn-studio/blocks/dashboard-shell-05"
import { useAuthStore } from "@/components/auth/use-auth-store"
import { LoginToast, useLoginToastStore } from "@/components/auth/login-toast"

const navigationItems = [
  { title: "主页", href: "/", icon: <Home className="h-4 w-4" />, isActive: true },
]

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  // Initialize auth state on mount
  const fetchCurrentUser = useAuthStore((state) => state.fetchCurrentUser)
  const { toast, hideToast } = useLoginToastStore()

  useEffect(() => {
    void fetchCurrentUser()
  }, [fetchCurrentUser])

  return (
    <html
      lang="zh"
      suppressHydrationWarning
      className="h-full antialiased font-sans"
    >
      <body className="flex h-dvh min-h-dvh flex-col overflow-hidden">
        <TooltipProvider>
          <ToastProvider>
            <ThemeProvider>
              <DashboardShell navigationItems={navigationItems}>
                {children}
              </DashboardShell>
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
      </body>
    </html>
  )
}
