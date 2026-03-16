"use client"

import { BellIcon, MenuIcon, Moon, Sun, Settings, Bot, Cloud } from 'lucide-react'
import { useTheme } from 'next-themes'
import Link from 'next/link'

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
import NotificationDropdown from '@/components/shadcn-studio/blocks/dropdown-notification'
import ProfileDropdown from '@/components/shadcn-studio/blocks/dropdown-profile'
import { useAuthStore } from '@/components/auth/use-auth-store'

type NavigationItem = {
  title: string
  href: string
  isActive?: boolean
}[]

const Navbar = ({
  navigationItems
}: {
  navigationItems: NavigationItem
}) => {
  const { resolvedTheme, setTheme } = useTheme()
  const { username, isLoggedIn } = useAuthStore()

  // Get avatar fallback based on auth state
  const avatarFallback = isLoggedIn && username ? username.charAt(0).toUpperCase() : '?'
  const avatarClassName = isLoggedIn
    ? 'size-9.5 rounded-lg bg-primary text-primary-foreground'
    : 'size-9.5 rounded-lg bg-muted text-muted-foreground'

  const toggleTheme = () => {
    setTheme(resolvedTheme === 'dark' ? 'light' : 'dark')
  }

  return (
    <header className='bg-background sticky top-0 z-50'>
      <div className='mx-auto flex max-w-7xl items-center justify-between px-6 py-4'>
        <Link href='/'>
          <div className='flex items-center gap-3'>
            <LogoSvg className='size-8' />
            <span className='text-xl font-semibold max-sm:hidden'>ArgusWing</span>
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
            <span className='sr-only'>Toggle theme</span>
          </Button>

          <DropdownMenu>
            <DropdownMenuTrigger render={<Button variant='outline' size='icon' />}>
              <Settings />
              <span className='sr-only'>Settings</span>
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

          <ProfileDropdown
            trigger={
              <Button variant='ghost' className='h-full rounded-lg p-0'>
                <Avatar className={avatarClassName}>
                  <AvatarImage src='' className='rounded-lg' />
                  <AvatarFallback>{avatarFallback}</AvatarFallback>
                </Avatar>
              </Button>
            }
          />

          <DropdownMenu>
            <DropdownMenuTrigger className='md:hidden' render={<Button variant='outline' size='icon' />}>
              <MenuIcon />
              <span className='sr-only'>Menu</span>
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
  )
}

export default Navbar
