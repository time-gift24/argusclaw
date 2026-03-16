'use client'

import { useState, type ReactElement } from 'react'

import {
  UserIcon,
  SettingsIcon,
  CreditCardIcon,
  UsersIcon,
  SquarePenIcon,
  CirclePlusIcon,
  LogOutIcon,
  LogInIcon
} from 'lucide-react'

import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger
} from '@/components/ui/dropdown-menu'
import { useAuthStore } from '@/components/auth/use-auth-store'
import { LoginDialog } from '@/components/auth/login-dialog'

type Props = {
  trigger: ReactElement
  defaultOpen?: boolean
  align?: 'start' | 'center' | 'end'
}

const getAvatarFallback = (username: string | null): string => {
  if (!username) return '?'
  return username.charAt(0).toUpperCase()
}

const ProfileDropdown = ({ trigger, defaultOpen, align = 'end' }: Props) => {
  const { username, isLoggedIn, logout } = useAuthStore()
  const [loginDialogOpen, setLoginDialogOpen] = useState(false)

  const handleLogout = async () => {
    await logout()
  }

  const handleLoginClick = () => {
    setLoginDialogOpen(true)
  }

  return (
    <>
      <DropdownMenu defaultOpen={defaultOpen}>
        <DropdownMenuTrigger render={trigger} />
        <DropdownMenuContent className='w-80' align={align || 'end'}>
          {isLoggedIn ? (
            <>
              <DropdownMenuLabel className='flex items-center gap-4 px-4 py-2.5 font-normal'>
                <div className='relative'>
                  <Avatar className='size-10'>
                    <AvatarFallback>{getAvatarFallback(username)}</AvatarFallback>
                  </Avatar>
                  <span className='ring-card absolute right-0 bottom-0 block size-2 rounded-full bg-green-600 ring-2' />
                </div>
                <div className='flex flex-1 flex-col items-start'>
                  <span className='text-foreground text-lg font-semibold'>{username}</span>
                </div>
              </DropdownMenuLabel>

              <DropdownMenuSeparator />

              <DropdownMenuGroup>
                <DropdownMenuItem className='px-4 py-2.5 text-base'>
                  <UserIcon className='text-foreground size-5' />
                  <span>My account</span>
                </DropdownMenuItem>
                <DropdownMenuItem className='px-4 py-2.5 text-base'>
                  <SettingsIcon className='text-foreground size-5' />
                  <span>Settings</span>
                </DropdownMenuItem>
                <DropdownMenuItem className='px-4 py-2.5 text-base'>
                  <CreditCardIcon className='text-foreground size-5' />
                  <span>Billing</span>
                </DropdownMenuItem>
              </DropdownMenuGroup>

              <DropdownMenuSeparator />

              <DropdownMenuGroup>
                <DropdownMenuItem className='px-4 py-2.5 text-base'>
                  <UsersIcon className='text-foreground size-5' />
                  <span>Manage team</span>
                </DropdownMenuItem>
                <DropdownMenuItem className='px-4 py-2.5 text-base'>
                  <SquarePenIcon className='text-foreground size-5' />
                  <span>Customization</span>
                </DropdownMenuItem>
                <DropdownMenuItem className='px-4 py-2.5 text-base'>
                  <CirclePlusIcon className='text-foreground size-5' />
                  <span>Add team account</span>
                </DropdownMenuItem>
              </DropdownMenuGroup>

              <DropdownMenuSeparator />

              <DropdownMenuItem
                variant='destructive'
                className='px-4 py-2.5 text-base'
                onSelect={handleLogout}
              >
                <LogOutIcon className='size-5' />
                <span>Logout</span>
              </DropdownMenuItem>
            </>
          ) : (
            <DropdownMenuItem className='px-4 py-2.5 text-base' onSelect={handleLoginClick}>
              <LogInIcon className='text-foreground size-5' />
              <span>Login</span>
            </DropdownMenuItem>
          )}
        </DropdownMenuContent>
      </DropdownMenu>

      <LoginDialog open={loginDialogOpen} onOpenChange={setLoginDialogOpen} />
    </>
  )
}

export default ProfileDropdown
