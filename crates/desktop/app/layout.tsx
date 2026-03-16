"use client"

import { useEffect } from "react"
import { Geist, Geist_Mono, Public_Sans } from "next/font/google"

import "./globals.css"
import { ThemeProvider } from "@/components/theme-provider"
import { TooltipProvider } from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
import Navbar from "@/components/shadcn-studio/blocks/navbar-component-06/navbar-component-06"
import { useAuthStore } from "@/components/auth/use-auth-store"

const publicSans = Public_Sans({ subsets: ["latin"], variable: "--font-sans" })

const fontMono = Geist_Mono({
  subsets: ["latin"],
  variable: "--font-mono",
})

const navigationItems = [
  { title: "Home", href: "/", isActive: true },
]

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  // Initialize auth state on mount
  const fetchCurrentUser = useAuthStore((state) => state.fetchCurrentUser)

  useEffect(() => {
    void fetchCurrentUser()
  }, [fetchCurrentUser])

  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={cn("antialiased", fontMono.variable, "font-sans", publicSans.variable)}
    >
      <body>
        <TooltipProvider>
          <ThemeProvider>
            <Navbar navigationItems={navigationItems} />
            <main className="flex-1">{children}</main>
          </ThemeProvider>
        </TooltipProvider>
      </body>
    </html>
  )
}
