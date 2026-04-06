import * as React from "react"
import { Link, useLocation } from "react-router-dom"
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
  BookOpen,
  Server,
} from "lucide-react"
import { useTheme } from "@/components/theme-provider"

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
  const location = useLocation()
  const pathname = location.pathname
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
          <Link to="/" className="flex items-center gap-2 overflow-hidden">
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
                to={item.href}
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
              to="/settings/agents"
              className="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground"
            >
              <Bot className="h-4 w-4" />
              智能体配置
            </Link>
            <Link
              to="/settings/providers"
              className="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground"
            >
              <Cloud className="h-4 w-4" />
              模型配置
            </Link>
            <Link
              to="/settings/mcp"
              className="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground"
            >
              <Server className="h-4 w-4" />
              MCP 配置
            </Link>
            <Link
              to="/settings/knowledge"
              className="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground"
            >
              <BookOpen className="h-4 w-4" />
              知识仓库
            </Link>
          </nav>
        </div>

        {/* Sidebar Footer */}
        <div className="border-t p-3 space-y-2">
          <Button
            variant="ghost"
            size="icon"
            onClick={toggleSidebar}
            className="w-full h-8"
          >
            {isSidebarCollapsed ? (
              <PanelLeft className="h-4 w-4" />
            ) : (
              <ChevronLeft className="h-4 w-4" />
            )}
          </Button>
        </div>
      </aside>

      {/* Main Content Area */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Header */}
        <header className="sticky top-0 z-50 border-b bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60">
          <div className="flex h-14 items-center px-6 gap-4">
            {/* Mobile menu toggle */}
            <Button
              variant="ghost"
              size="icon"
              className="md:hidden"
              onClick={() => setIsMobileMenuOpen(true)}
            >
              <Menu className="h-5 w-5" />
            </Button>

            {/* Search */}
            <div className="flex-1 max-w-md">
              <div className="relative">
                <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
                <Input
                  placeholder="搜索..."
                  className="pl-8 h-9 text-sm"
                />
              </div>
            </div>

            {/* Header Actions */}
            <div className="flex items-center gap-2">
              <Button
                variant="ghost"
                size="icon"
                onClick={toggleTheme}
                className="h-9 w-9"
              >
                {resolvedTheme === "dark" ? (
                  <Sun className="h-4 w-4" />
                ) : (
                  <Moon className="h-4 w-4" />
                )}
              </Button>

              {/* Notifications */}
              <Button variant="ghost" size="icon" className="h-9 w-9">
                <Bell className="h-4 w-4" />
              </Button>

              {/* User Menu */}
              {isLoggedIn ? (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button variant="ghost" className="relative h-9 w-9 rounded-full">
                      <Avatar className="h-9 w-9">
                        <AvatarImage alt={username} />
                        <AvatarFallback>
                          <User className="h-4 w-4" />
                        </AvatarFallback>
                      </Avatar>
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-56">
                    <DropdownMenuLabel>
                      <div className="flex flex-col space-y-1">
                        <p className="text-sm font-medium">{username}</p>
                      </div>
                    </DropdownMenuLabel>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem asChild>
                      <Link to="/settings">
                        <Settings className="mr-2 h-4 w-4" />
                        设置
                      </Link>
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem onClick={handleLogout}>
                      <LogOut className="mr-2 h-4 w-4" />
                      退出登录
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              ) : (
                <Button variant="ghost" size="sm" onClick={openLoginDialog}>
                  <User className="mr-2 h-4 w-4" />
                  登录
                </Button>
              )}
            </div>
          </div>
        </header>

        {/* Page Content */}
        <main className="flex-1 overflow-auto">
          <div className="container mx-auto py-6">
            {children}
          </div>
        </main>
      </div>

      {/* Mobile Menu Overlay */}
      {isMobileMenuOpen && (
        <div className="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm md:hidden">
          <div className="fixed inset-y-0 left-0 z-50 w-64 border-r bg-background p-6 shadow-lg animate-in slide-in-from-left">
             <div className="flex items-center justify-between mb-8">
                <Link to="/" className="flex items-center gap-2">
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
                   to={item.href}
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
                  to="/settings/agents"
                  onClick={() => setIsMobileMenuOpen(false)}
                  className="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground"
                >
                  <Bot className="h-4 w-4" />
                  智能体配置
                </Link>
                <Link
                  to="/settings/providers"
                  onClick={() => setIsMobileMenuOpen(false)}
                  className="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground"
                >
                  <Cloud className="h-4 w-4" />
                  模型配置
                </Link>
                <Link
                  to="/settings/mcp"
                  onClick={() => setIsMobileMenuOpen(false)}
                  className="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground"
                >
                  <Server className="h-4 w-4" />
                  MCP 配置
                </Link>
                <Link
                  to="/settings/knowledge"
                  onClick={() => setIsMobileMenuOpen(false)}
                  className="flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground"
                >
                  <BookOpen className="h-4 w-4" />
                  知识仓库
                </Link>
             </nav>
          </div>
        </div>
      )}
    </div>
  )
}
