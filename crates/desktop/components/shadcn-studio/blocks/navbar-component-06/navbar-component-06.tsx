"use client"

import { useState, useEffect, useMemo } from 'react'
import { BellIcon, MenuIcon, Moon, Sun, Settings, Bot, Cloud, ChevronRight, ArrowLeft } from 'lucide-react'
import { useTheme } from 'next-themes'
import Link from 'next/link'
import { usePathname, useRouter } from 'next/navigation'

import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from '@/components/ui/dropdown-menu'
import {
  NavigationMenu,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  navigationMenuTriggerStyle
} from '@/components/ui/navigation-menu'

import LogoSvg from '@/assets/svg/logo'
import { LoginDialog } from '@/components/auth/login-dialog'
import NotificationDropdown from '@/components/shadcn-studio/blocks/dropdown-notification'
import ProfileDropdown from '@/components/shadcn-studio/blocks/dropdown-profile'
import { useAuthStore } from '@/components/auth/use-auth-store'
import { agents, providers } from '@/lib/tauri'

interface BreadcrumbItem {
  label: string
  href?: string
}

type NavigationItem = {
  title: string
  href: string
  isActive?: boolean
}[]

// Generate breadcrumb items based on current path
function useBreadcrumbItems(pathname: string): BreadcrumbItem[] {
  const [agentNames, setAgentNames] = useState<Record<string, string>>({})
  const [providerNames, setProviderNames] = useState<Record<string, string>>({})
  const [editIds, setEditIds] = useState<{ agentId: string | null; providerId: string | null }>({
    agentId: null,
    providerId: null,
  })

  // Load agent and provider names for breadcrumb display
  useEffect(() => {
    const loadNames = async () => {
      try {
        const [agentList, providerList] = await Promise.all([
          agents.list(),
          providers.list(),
        ])
        const agentNameMap: Record<string, string> = {}
        const providerNameMap: Record<string, string> = {}
        for (const agent of agentList) {
          agentNameMap[agent.id] = agent.display_name
        }
        for (const provider of providerList) {
          providerNameMap[provider.id] = provider.display_name
        }
        setAgentNames(agentNameMap)
        setProviderNames(providerNameMap)
      } catch (error) {
        console.error("Failed to load names:", error)
      }
    }
    loadNames()
  }, [])

  useEffect(() => {
    if (typeof window === "undefined") {
      return
    }

    const params = new URLSearchParams(window.location.search)
    setEditIds({
      agentId: pathname === "/settings/agents/edit" ? params.get("id") : null,
      providerId: pathname === "/settings/providers/edit" ? params.get("id") : null,
    })
  }, [pathname])

  return useMemo(() => {
    const items: BreadcrumbItem[] = []

    // Settings routes
    if (pathname.startsWith("/settings")) {
      items.push({ label: "设置", href: "/settings" })

      if (pathname.startsWith("/settings/providers")) {
        items.push({ label: "LLM 提供者", href: "/settings/providers" })

        if (pathname === "/settings/providers/new") {
          items.push({ label: "新建" })
        } else if (pathname === "/settings/providers/edit") {
          const providerId = editIds.providerId
          if (providerId) {
            const providerName = providerNames[providerId] || providerId
            items.push({ label: providerName })
          }
        } else {
          const match = pathname.match(/^\/settings\/providers\/([^/]+)$/)
          if (match) {
            const providerId = match[1]
            const providerName = providerNames[providerId] || providerId
            items.push({ label: providerName })
          }
        }
      } else if (pathname.startsWith("/settings/agents")) {
        items.push({ label: "智能体", href: "/settings/agents" })

        if (pathname === "/settings/agents/new") {
          items.push({ label: "新建" })
        } else if (pathname === "/settings/agents/edit") {
          const agentId = editIds.agentId
          if (agentId) {
            const agentName = agentNames[agentId] || agentId
            items.push({ label: agentName })
          }
        } else {
          const match = pathname.match(/^\/settings\/agents\/([^/]+)$/)
          if (match) {
            const agentId = match[1]
            const agentName = agentNames[agentId] || agentId
            items.push({ label: agentName })
          }
        }
      }
    }

    return items
  }, [pathname, editIds, agentNames, providerNames])
}

const Navbar = ({
  navigationItems
}: {
  navigationItems: NavigationItem
}) => {
  const { resolvedTheme, setTheme } = useTheme()
  const { username, isLoggedIn } = useAuthStore()
  const [loginDialogOpen, setLoginDialogOpen] = useState(false)
  const pathname = usePathname()
  const router = useRouter()
  const breadcrumbItems = useBreadcrumbItems(pathname)

  // Get avatar fallback based on auth state
  const avatarFallback = isLoggedIn && username ? username.charAt(0).toUpperCase() : '?'
  const avatarClassName = isLoggedIn
    ? 'size-8 rounded-md bg-primary text-primary-foreground text-sm'
    : 'size-8 rounded-md bg-muted text-muted-foreground text-sm'

  const toggleTheme = () => {
    setTheme(resolvedTheme === 'dark' ? 'light' : 'dark')
  }

  const openLoginDialog = () => {
    setLoginDialogOpen(true)
  }

  return (
    <>
      <header className='bg-background sticky top-0 z-50'>
        <div className='mx-auto flex max-w-7xl items-center justify-between px-6 py-4'>
          <Link href='/'>
            <div className='flex items-center gap-3'>
              <LogoSvg className='size-8' />
              <span className='text-lg font-semibold max-sm:hidden'>ArgusWing</span>
            </div>
          </Link>

          <NavigationMenu className='max-md:hidden'>
            <NavigationMenuList className='gap-8'>
              {navigationItems.map((item, index) => (
                <NavigationMenuItem key={index} className='has-[[data-active]]:text-foreground text-muted-foreground'>
                  <NavigationMenuLink href={item.href} className={navigationMenuTriggerStyle()} active={item.isActive}>
                    {item.title}
                  </NavigationMenuLink>
                </NavigationMenuItem>
              ))}
            </NavigationMenuList>
          </NavigationMenu>

          <div className='flex items-center gap-3'>
            <Button variant='outline' size='icon' onClick={toggleTheme}>
              {resolvedTheme === 'dark' ? <Moon /> : <Sun />}
              <span className='sr-only'>切换主题</span>
            </Button>

            <DropdownMenu>
              <DropdownMenuTrigger render={<Button variant='outline' size='icon' />}>
                <Settings />
                <span className='sr-only'>设置</span>
              </DropdownMenuTrigger>
              <DropdownMenuContent align='end' className='min-w-48'>
                <DropdownMenuItem>
                  <Link href='/settings/agents' className='flex items-center gap-2 w-full'>
                    <Bot className='h-4 w-4' />
                    <span>Agent 配置</span>
                  </Link>
                </DropdownMenuItem>
                <DropdownMenuItem>
                  <Link href='/settings/providers' className='flex items-center gap-2 w-full'>
                    <Cloud className='h-4 w-4' />
                    <span>LLMProvider 配置</span>
                  </Link>
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>

            <NotificationDropdown
              trigger={
                <Button variant='outline' size='icon' className='relative'>
                  <BellIcon />
                  <span className='bg-destructive absolute -top-0.5 -right-0.5 size-2 rounded-full' />
                </Button>
              }
            />

            {isLoggedIn ? (
              <ProfileDropdown
                trigger={
                  <Button variant='ghost' className='h-full rounded-lg p-0'>
                    <Avatar className={avatarClassName}>
                      <AvatarImage src='' className='rounded-lg' />
                      <AvatarFallback>{avatarFallback}</AvatarFallback>
                    </Avatar>
                  </Button>
                }
                onLoginRequested={openLoginDialog}
              />
            ) : (
              <Button
                variant='ghost'
                className='h-8 rounded-md p-0'
                onClick={openLoginDialog}
                aria-label='打开登录弹窗'
              >
                <Avatar className={avatarClassName}>
                  <AvatarImage src='' className='rounded-md' />
                  <AvatarFallback>{avatarFallback}</AvatarFallback>
                </Avatar>
              </Button>
            )}

            <LoginDialog open={loginDialogOpen} onOpenChange={setLoginDialogOpen} />

            <DropdownMenu>
              <DropdownMenuTrigger className='md:hidden' render={<Button variant='outline' size='icon' />}>
                <MenuIcon />
                <span className='sr-only'>菜单</span>
              </DropdownMenuTrigger>
              <DropdownMenuContent className='w-56' align='end'>
                {navigationItems.map((item, index) => (
                  <DropdownMenuItem key={index}>
                    <Link href={item.href}>{item.title}</Link>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
      </header>

      {/* Sub navigation with breadcrumb */}
      {breadcrumbItems.length > 0 && (
        <div className="border-b bg-background">
          <div className="mx-auto flex max-w-7xl items-center gap-2 px-6 py-2">
            {breadcrumbItems.length >= 3 && (
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6"
                onClick={() => {
                  const parentHref = breadcrumbItems[breadcrumbItems.length - 2]?.href
                  if (parentHref) {
                    router.push(parentHref)
                  }
                }}
              >
                <ArrowLeft className="h-4 w-4" />
              </Button>
            )}
            <nav className="flex items-center gap-1 text-sm text-muted-foreground">
              {breadcrumbItems.map((item, index) => (
                <span key={index} className="flex items-center gap-1">
                  {index > 0 && <ChevronRight className="h-4 w-4" />}
                  {item.href ? (
                    <Link href={item.href} className="hover:text-foreground transition-colors">
                      {item.label}
                    </Link>
                  ) : (
                    <span className="text-foreground">{item.label}</span>
                  )}
                </span>
              ))}
            </nav>
          </div>
        </div>
      )}
    </>
  )
}

export default Navbar
