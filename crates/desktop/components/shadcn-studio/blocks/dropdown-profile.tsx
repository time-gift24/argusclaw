'use client'

import {
  cloneElement,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type ReactElement,
} from 'react'

import { LogOutIcon } from 'lucide-react'

import { useAuthStore } from '@/components/auth/use-auth-store'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

type TriggerElementProps = {
  onClick?: (event: MouseEvent<HTMLElement>) => void
  onKeyDown?: (event: KeyboardEvent<HTMLElement>) => void
  role?: string
  tabIndex?: number
  'aria-label'?: string
}

type Props = {
  trigger: ReactElement<TriggerElementProps>
  defaultOpen?: boolean
  align?: 'start' | 'center' | 'end'
  onLoginRequested?: () => void
}

const ProfileDropdown = ({ trigger, defaultOpen, onLoginRequested }: Props) => {
  const { username, isLoggedIn, logout } = useAuthStore()
  const [dialogOpen, setDialogOpen] = useState(defaultOpen ?? false)
  const [loggingOut, setLoggingOut] = useState(false)

  const openLoginDialog = () => {
    window.requestAnimationFrame(() => {
      onLoginRequested?.()
    })
  }

  const handleTriggerActivation = () => {
    if (isLoggedIn) {
      setDialogOpen(true)
      return
    }

    openLoginDialog()
  }

  const handleTriggerClick = (event: MouseEvent<HTMLElement>) => {
    trigger.props.onClick?.(event)
    if (event.defaultPrevented) return
    handleTriggerActivation()
  }

  const handleTriggerKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    trigger.props.onKeyDown?.(event)
    if (event.defaultPrevented) return

    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      handleTriggerActivation()
    }
  }

  const handleLogoutConfirm = async () => {
    setLoggingOut(true)

    try {
      await logout()
      setDialogOpen(false)
    } finally {
      setLoggingOut(false)
    }
  }

  return (
    <>
      {cloneElement(trigger, {
        onClick: handleTriggerClick,
        onKeyDown: handleTriggerKeyDown,
        role: trigger.props.role ?? 'button',
        tabIndex: trigger.props.tabIndex ?? 0,
        'aria-label': trigger.props['aria-label'] ?? (isLoggedIn ? '打开退出登录确认弹窗' : '打开登录弹窗'),
      })}

      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className='gap-3 p-5 sm:max-w-sm' showCloseButton={false}>
          <DialogHeader className='gap-1.5'>
            <DialogTitle className='text-base font-semibold'>确认退出登录</DialogTitle>
            <DialogDescription className='text-sm leading-6'>
              {username
                ? `退出 ${username} 后，需要重新登录才能继续使用 ArgusWing。`
                : '退出后，需要重新登录才能继续使用 ArgusWing。'}
            </DialogDescription>
          </DialogHeader>

          <DialogFooter className='gap-2 sm:gap-0'>
            <Button variant='outline' onClick={() => setDialogOpen(false)} disabled={loggingOut}>
              取消
            </Button>
            <Button variant='destructive' onClick={handleLogoutConfirm} disabled={loggingOut}>
              <LogOutIcon className='size-4' />
              <span>{loggingOut ? '正在退出...' : '退出登录'}</span>
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

export default ProfileDropdown
