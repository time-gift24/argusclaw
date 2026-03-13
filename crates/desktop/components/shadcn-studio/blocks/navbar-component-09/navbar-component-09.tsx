import { BellIcon, MenuIcon } from 'lucide-react'

import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import {
  NavigationMenu,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuList,
  navigationMenuTriggerStyle
} from '@/components/ui/navigation-menu'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger
} from '@/components/ui/dropdown-menu'

import Logo from '@/components/shadcn-studio/logo'
import NotificationDropdown from '@/components/shadcn-studio/blocks/dropdown-notification'
import ProfileDropdown from '@/components/shadcn-studio/blocks/dropdown-profile'

type NavigationItem = {
  title: string
  href: string
  isActive?: boolean
}[]

const Navbar = ({ navigationData }: { navigationData: NavigationItem }) => {
  return (
    <header className='bg-background sticky top-0 z-50'>
      <div className='mx-auto flex max-w-7xl items-center justify-between gap-4 px-4 py-3 sm:px-6'>
        <div className='flex items-center gap-4 lg:gap-10'>
          <a href='#'>
            <Logo className='gap-3' />
          </a>
          <NavigationMenu className='hidden md:block'>
            <NavigationMenuList className='gap-4'>
              {navigationData.map((item, index) => (
                <NavigationMenuItem key={index} className='has-[[data-active]]:text-foreground text-muted-foreground'>
                  <NavigationMenuLink href={item.href} className={navigationMenuTriggerStyle()} active={item.isActive}>
                    {item.title}
                  </NavigationMenuLink>
                </NavigationMenuItem>
              ))}
            </NavigationMenuList>
          </NavigationMenu>
        </div>

        <div className='flex items-center gap-3 md:gap-4'>
          <NotificationDropdown
            trigger={
              <span className='relative inline-flex items-center justify-center rounded-md border px-2.5 py-1.5'>
                <BellIcon className='size-4' />
                <span className='bg-destructive absolute -top-0.5 -right-0.5 size-2 rounded-full' />
              </span>
            }
          />

          <ProfileDropdown
            trigger={
              <span className='inline-flex h-full items-center gap-3 p-0 sm:pr-1'>
                <Avatar className='size-8.5 rounded-md'>
                  <AvatarImage src='https://cdn.shadcnstudio.com/ss-assets/avatar/avatar-1.png' />
                  <AvatarFallback>JD</AvatarFallback>
                </Avatar>
                <div className='hidden flex-col items-start gap-0.5 lg:flex'>
                  <span className='text-lg font-semibold'>John Doe</span>
                  <span className='text-muted-foreground text-sm font-normal'>UI/UX Designer</span>
                </div>
              </span>
            }
          />

          <DropdownMenu>
            <DropdownMenuTrigger className='flex items-center justify-center rounded-md border outline-none px-2.5 py-1.5 md:hidden'>
              <MenuIcon className='size-4' />
              <span className='sr-only'>Menu</span>
            </DropdownMenuTrigger>
            <DropdownMenuContent className='w-56' align='end'>
              <DropdownMenuGroup>
                {navigationData.map((item, index) => (
                  <DropdownMenuItem key={index}>
                    <a href={item.href}>{item.title}</a>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuGroup>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
    </header>
  )
}

export default Navbar
