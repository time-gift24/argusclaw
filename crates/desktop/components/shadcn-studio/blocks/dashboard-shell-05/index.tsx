"use client"

import * as React from "react"
import Link from "next/link"
import { usePathname } from "next/navigation"
import {
  Bell,
  ChevronLeft,
  ChevronRight,
  Menu,
  Search,
  Settings,
  User,
  LogOut,
  Bot,
  Cloud,
  Home,
  Sun,
  Moon,
  PanelLeft,
} from "lucide-react"
import { useTheme } from "next-themes"

import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar"
import { Separator } from "@/components/ui/separator"
import LogoSvg from "@/assets/svg/logo"
import { useAuthStore } from "@/components/auth/use-auth-store"
import { LoginDialog } from "@/components/auth/login-dialog"

interface DashboardShellProps {
  children: React.ReactNode
  navigationItems: {
    title: string
    href: string
    icon?: React.ReactNode
    isActive?: boolean
  }[]
}

export default function DashboardShell({
  children,
  navigationItems,
}: DashboardShellProps) {
  const [isSidebarCollapsed, setIsSidebarCollapsed] = React.useState(false)
  const [isMobileMenuOpen, setIsMobileMenuOpen] = React.useState(false)
  const [loginDialogOpen, setLoginDialogOpen] = React.useState(false)
  const { resolvedTheme, setTheme } = useTheme()
  const pathname = usePathname()
  const { username, isLoggedIn, logout } = useAuthStore()

  const toggleSidebar = () => setIsSidebarCollapsed(!isSidebarCollapsed)
  const toggleTheme = () => setTheme(resolvedTheme === "dark" ? "light" : "dark")
  const openLoginDialog = () => setLoginDialogOpen(true)

  const handleLogout = async () => {
    await logout()
  }

  return (
    <div className="flex h-screen w-full bg-background overflow-hidden">
      {/* Desktop Sidebar */}
      <aside
        className={cn(
          "hidden md:flex flex-col border-r bg-muted/30 transition-all duration-300 ease-in-out",
          isSidebarCollapsed ? "w-16" : "w-64"
        )}
      >
        <div className="flex h-14 items-center px-4 border-b">
          <Link href="/" className="flex items-center gap-2 overflow-hidden">
            <LogoSvg className="h-8 w-8 shrink-0" />
            {!isSidebarCollapsed && (
              <span className="font-semibold text-sm whitespace-nowrap">ArgusWing</span>
            )}
          </Link>
        </div>

        <div className="flex-1 py-4 px-2 overflow-y-auto">
          <nav className="grid gap-1">
            {navigationItems.map((item, index) => (
              <Link
                key={index}
                href={item.href}
                className={cn(
                  "flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground",
                  pathname === item.href || item.isActive
                    ? "bg-accent text-accent-foreground"
                    : "text-muted-foreground"
                )}
              >
                <span className="shrink-0">
                  {item.icon || <Home className="h-4 w-4" />}
                </span>
                {!isSidebarCollapsed && (
                  <span className="whitespace-nowrap">{item.title}</span>
                )}
              </Link>
            ))}
          </nav>

          <Separator className="my-4 mx-2" />

          <nav className="grid gap-1">
            <Link
              href="/settings/agents"
              className={cn(
                "flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground",
                pathname.startsWith("/settings/agents")
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground"
              )}
            >
              <Bot className="h-4 w-4 shrink-0" />
              {!isSidebarCollapsed && <span className="whitespace-nowrap">智能体配置</span>}
            </Link>
            <Link
              href="/settings/providers"
              className={cn(
                "flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground",
                pathname.startsWith("/settings/providers")
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground"
              )}
            >
              <Cloud className="h-4 w-4 shrink-0" />
              {!isSidebarCollapsed && <span className="whitespace-nowrap">模型配置</span>}
            </Link>
          </nav>
        </div>
      </aside>

      {/* Main Area */}
      <div className="flex flex-1 flex-col overflow-hidden">
        {/* Header */}
        <header className="flex h-14 items-center justify-between border-b bg-background px-4 md:px-6">
          <div className="flex items-center gap-4">
            <Button
              variant="ghost"
              size="icon"
              className="hidden md:flex"
              onClick={toggleSidebar}
            >
              <PanelLeft className="h-4 w-4" />
            </Button>

            <Button
              variant="ghost"
              size="icon"
              className="md:hidden"
              onClick={() => setIsMobileMenuOpen(!isMobileMenuOpen)}
            >
              <Menu className="h-5 w-5" />
            </Button>

            <div className="relative w-64">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                type="search"
                placeholder="搜索..."
                className="w-full bg-muted/50 pl-8 h-9 text-sm focus-visible:ring-1 border-none shadow-none"
              />
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Button variant="ghost" size="icon" onClick={toggleTheme}>
              {resolvedTheme === "dark" ? <Moon className="h-4 w-4" /> : <Sun className="h-4 w-4" />}
            </Button>

            <Button variant="ghost" size="icon" className="relative">
              <Bell className="h-4 w-4" />
              <span className="absolute top-2 right-2 h-1.5 w-1.5 rounded-full bg-primary" />
            </Button>

            <Separator orientation="vertical" className="h-6 mx-1" />

            {isLoggedIn ? (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="ghost" size="sm" className="relative h-8 w-8 rounded-full">
                    <Avatar className="h-8 w-8">
                      <AvatarImage src="" />
                      <AvatarFallback className="text-xs bg-primary text-primary-foreground">
                        {username ? username.charAt(0).toUpperCase() : <User className="h-4 w-4" />}
                      </AvatarFallback>
                    </Avatar>
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-56">
                  <DropdownMenuLabel>
                    <div className="flex flex-col space-y-1">
                      <p className="text-sm font-medium leading-none">{username}</p>
                      <p className="text-xs leading-none text-muted-foreground">已登录</p>
                    </div>
                  </DropdownMenuLabel>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem>
                    <Settings className="mr-2 h-4 w-4" />
                    <span>设置</span>
                  </DropdownMenuItem>
                  <DropdownMenuItem>
                    <User className="mr-2 h-4 w-4" />
                    <span>个人中心</span>
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem className="text-destructive focus:bg-destructive/10 focus:text-destructive" onClick={handleLogout}>
                    <LogOut className="mr-2 h-4 w-4" />
                    <span>退出登录</span>
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            ) : (
              <Button
                variant="ghost"
                size="sm"
                className="relative h-8 w-8 rounded-full"
                onClick={openLoginDialog}
              >
                <Avatar className="h-8 w-8">
                  <AvatarFallback className="text-xs bg-muted text-muted-foreground">
                    <User className="h-4 w-4" />
                  </AvatarFallback>
                </Avatar>
              </Button>
            )}

            <LoginDialog open={loginDialogOpen} onOpenChange={setLoginDialogOpen} />
          </div>
        </header>

        {/* Content Area */}
        <main className="flex-1 relative flex flex-col min-h-0 overflow-hidden">
          <div className="w-full flex-1 flex flex-col min-h-0">
            {children}
          </div>
        </main>
      </div>

      {/* Mobile Menu Overlay */}
      {isMobileMenuOpen && (
        <div className="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm md:hidden">
          <div className="fixed inset-y-0 left-0 z-50 w-64 border-r bg-background p-6 shadow-lg animate-in slide-in-from-left">
             <div className="flex items-center justify-between mb-8">
                <Link href="/" className="flex items-center gap-2">
                  <LogoSvg className="h-8 w-8" />
                  <span className="font-semibold">ArgusWing</span>
                </Link>
                <Button variant="ghost" size="icon" onClick={() => setIsMobileMenuOpen(false)}>
                  <ChevronLeft className="h-5 w-5" />
                </Button>
             </div>
             <nav className="grid gap-2">
               {navigationItems.map((item, index) => (
                 <Link
                   key={index}
                   href={item.href}
                   onClick={() => setIsMobileMenuOpen(false)}
                   className={cn(
                     "flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors hover:bg-accent hover:text-accent-foreground",
                     pathname === item.href || item.isActive
                       ? "bg-accent text-accent-foreground"
                       : "text-muted-foreground"
                   )}
                 >
                   {item.icon || <Home className="h-4 w-4" />}
                   {item.title}
                 </Link>
               ))}
               <Separator className="my-2" />
               <Link
                  href="/settings/agents"
                  onClick={() => setIsMobileMenuOpen(false)}
                  className="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground"
                >
                  <Bot className="h-4 w-4" />
                  智能体配置
                </Link>
                <Link
                  href="/settings/providers"
                  onClick={() => setIsMobileMenuOpen(false)}
                  className="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground"
                >
                  <Cloud className="h-4 w-4" />
                  模型配置
                </Link>
             </nav>
          </div>
        </div>
      )}
    </div>
  )
}
