'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useAuthStore } from './use-auth-store';

interface LoginDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const getLoginErrorMessage = (message?: string): string => {
  if (!message) {
    return '登录时出现异常，请稍后重试。';
  }

  const normalizedMessage = message.toLowerCase();

  if (
    normalizedMessage.includes('invalid password') ||
    normalizedMessage.includes('user not found')
  ) {
    return '用户名或密码错误，请重试。';
  }

  if (normalizedMessage.includes('username is required')) {
    return '请输入用户名。';
  }

  if (normalizedMessage.includes('password is required')) {
    return '请输入密码。';
  }

  return `系统错误：${message}`;
};

export function LoginDialog({ open, onOpenChange }: LoginDialogProps) {
  const router = useRouter();
  const { login } = useAuthStore();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [isLoading, setIsLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setIsLoading(true);

    try {
      if (!username.trim()) {
        setError('请输入用户名。');
        setIsLoading(false);
        return;
      }
      if (!password) {
        setError('请输入密码。');
        setIsLoading(false);
        return;
      }

      const result = await login(username.trim(), password);
      if (result.success) {
        onOpenChange(false);
        resetForm();
        router.push('/settings/providers/new');
      } else {
        setError(getLoginErrorMessage(result.error));
      }
    } finally {
      setIsLoading(false);
    }
  };

  const resetForm = () => {
    setUsername('');
    setPassword('');
    setError('');
  };

  const handleOpenChange = (newOpen: boolean) => {
    if (!newOpen) {
      resetForm();
    }
    onOpenChange(newOpen);
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="gap-3 p-5 sm:max-w-sm">
        <DialogHeader className="gap-1.5">
          <DialogTitle className="text-base font-semibold">登录</DialogTitle>
          <DialogDescription className="text-sm leading-6">
            输入用户名和密码后继续。
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-3.5">
          <div className="space-y-2">
            <Label htmlFor="username" className="text-sm">用户名</Label>
            <Input
              id="username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="请输入用户名"
              disabled={isLoading}
              maxLength={50}
              autoFocus
              className="h-9 px-3 text-sm md:text-sm"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="password" className="text-sm">密码</Label>
            <Input
              id="password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="请输入密码"
              disabled={isLoading}
              maxLength={100}
              className="h-9 px-3 text-sm md:text-sm"
            />
          </div>

          {error && <p className="text-sm leading-6 text-destructive">{error}</p>}

          <Button type="submit" className="h-9 w-full text-sm md:text-sm" disabled={isLoading}>
            {isLoading ? '请稍候...' : '登录'}
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  );
}
