"use client"

import { Geist, Geist_Mono, Public_Sans } from "next/font/google"

import "./globals.css"
import { ThemeProvider } from "@/components/theme-provider"
import { TooltipProvider } from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
import Navbar from "@/components/shadcn-studio/blocks/navbar-component-06/navbar-component-06"

const publicSans = Public_Sans({ subsets: ["latin"], variable: "--font-sans" })

const fontMono = Geist_Mono({
  subsets: ["latin"],
  variable: "--font-mono",
})

const navigationData = [
  { title: "Home", href: "/" },
  { title: "Marketing", href: "/marketing" },
  { title: "Contact", href: "/contact" },
  { title: "Projects", href: "/projects" },
  { title: "Sales", href: "/sales" },
  { title: "Team", href: "/team" },
  { title: "Tasks", href: "/tasks" },
  { title: "Blog", href: "/blog" },
]

const navigationItems = [
  { title: "Home", href: "/" },
  { title: "Marketing", href: "/marketing", isActive: true },
  { title: "Contact", href: "/contact" },
]

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={cn("antialiased", fontMono.variable, "font-sans", publicSans.variable)}
    >
      <body>
        <TooltipProvider>
          <ThemeProvider>
            <Navbar navigationData={navigationData} navigationItems={navigationItems} />
            <main className="flex-1">{children}</main>
          </ThemeProvider>
        </TooltipProvider>
      </body>
    </html>
  )
}
