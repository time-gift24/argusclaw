"use client"

import { BellIcon, MenuIcon, SearchIcon, Moon, Sun, Settings, Bot, Cloud } from 'lucide-react'
import { useTheme } from 'next-themes'

import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import {
  NavigationMenu,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  navigationMenuTriggerStyle
} from '@/components/ui/navigation-menu'
import { Separator } from '@/components/ui/separator'

import LogoSvg from '@/assets/svg/logo'
import NotificationDropdown from '@/components/shadcn-studio/blocks/dropdown-notification'
import ProfileDropdown from '@/components/shadcn-studio/blocks/dropdown-profile'

type NavigationData = {
  title: string
  href: string
}[]

type NavigationItem = {
  title: string
  href: string
  isActive?: boolean
}[]

const Navbar = ({
  navigationData,
  navigationItems
}: {
  navigationData: NavigationData
  navigationItems: NavigationItem
}) => {
  const { resolvedTheme, setTheme } = useTheme()

  const toggleTheme = () => {
    setTheme(resolvedTheme === 'dark' ? 'light' : 'dark')
  }

  return (
    <header className='bg-background sticky top-0 z-50'>
      <div className='mx-auto flex max-w-7xl items-center justify-between gap-4 px-4 py-3 sm:px-6'>
        <a href='#'>
          <div className='flex items-center'>
            <LogoSvg className='size-8' />
            <span className='ml-3 text-xl font-semibold max-sm:hidden'>shadcn/studio</span>
          </div>
        </a>

        <NavigationMenu className='max-md:hidden'>
          <NavigationMenuList className='gap-10'>
            {navigationItems.map((item, index) => (
              <NavigationMenuItem key={index} className='has-[[data-active]]:text-foreground text-muted-foreground'>
                <NavigationMenuLink href={item.href} className={navigationMenuTriggerStyle()} active={item.isActive}>
                  {item.title}
                </NavigationMenuLink>
              </NavigationMenuItem>
            ))}
          </NavigationMenuList>
        </NavigationMenu>

        <div className='flex items-center gap-2 md:gap-4'>
          <Button variant='outline' size='icon' className='md:hidden'>
            <SearchIcon />
            <span className='sr-only'>Search</span>
          </Button>

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
                <Bot />
                <span>Agent 配置</span>
              </DropdownMenuItem>
              <DropdownMenuItem>
                <Cloud />
                <span>LLMProvider 配置</span>
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
                <Avatar className='size-9.5 rounded-lg'>
                  <AvatarImage
                    src='https://cdn.shadcnstudio.com/ss-assets/avatar/avatar-1.png'
                    className='rounded-lg'
                  />
                  <AvatarFallback>JD</AvatarFallback>
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
              {navigationData.map((item, index) => (
                <DropdownMenuGroup key={index}>
                  <DropdownMenuItem>
                    <a href={item.href}>{item.title}</a>
                  </DropdownMenuItem>

                  {index == 2 && <DropdownMenuSeparator />}
                </DropdownMenuGroup>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      <Separator className='max-md:hidden' />

      <div className='mx-auto flex max-w-7xl items-center justify-between gap-8 px-4 py-3 max-md:hidden sm:px-6'>
        <div className='text-muted-foreground flex items-center gap-10 font-medium'>
          {navigationData.slice(3).map((item, index) => (
            <a key={index} href={item.href} className='hover:text-foreground'>
              {item.title}
            </a>
          ))}
        </div>

        <div className='text-muted-foreground flex items-center gap-3'>
          <Input type='text' placeholder='Type here' className='max-md:hidden' />
          <Button size='icon'>
            <SearchIcon />
            <span className='sr-only'>Search</span>
          </Button>
        </div>
      </div>
    </header>
  )
}

export default Navbar
