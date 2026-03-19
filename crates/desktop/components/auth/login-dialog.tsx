'use client';

import { useState, useEffect, useCallback } from 'react';
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
import { useLoginToastStore } from './login-toast';

interface LoginDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const getSetupErrorMessage = (message?: string): string => {
  if (!message) {
    return '创建账号时出现异常，请稍后重试。';
  }

  const normalizedMessage = message.toLowerCase();

  if (normalizedMessage.includes('user already exists')) {
    return '账号已创建，请直接登录。';
  }

  if (normalizedMessage.includes('username is required')) {
    return '请输入用户名。';
  }

  if (normalizedMessage.includes('password is required')) {
    return '请输入密码。';
  }

  if (normalizedMessage.includes('password must be at least')) {
    return '密码至少需要 4 个字符。';
  }

  return `系统错误：${message}`;
};

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
  const { checkHasUser, setupAccount, login } = useAuthStore();
  const { showToast } = useLoginToastStore();
  const [mode, setMode] = useState<'setup' | 'login'>('setup');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [isCheckingMode, setIsCheckingMode] = useState(true);

  // Determine mode when dialog opens
  const checkMode = useCallback(async () => {
    if (open) {
      setIsCheckingMode(true);
      const hasUser = await checkHasUser();
      setMode(hasUser ? 'login' : 'setup');
      setIsCheckingMode(false);
    }
  }, [open, checkHasUser]);

  useEffect(() => {
    checkMode();
  }, [checkMode]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setIsLoading(true);

    try {
      if (mode === 'setup') {
        // Validation
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
        if (password.length < 4) {
          setError('密码至少需要 4 个字符。');
          setIsLoading(false);
          return;
        }

        const result = await setupAccount(username.trim(), password);
        if (result.success) {
          onOpenChange(false);
          resetForm();
          showToast('账号创建成功！请填写 LLM Provider 配置并测试连接。', 'success');
          router.push('/settings/providers/1');
        } else {
          const nextError = getSetupErrorMessage(result.error);
          setError(nextError);

          if (nextError === '账号已创建，请直接登录。') {
            setMode('login');
          }
        }
      } else {
        // Login mode
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
          showToast('登录成功！', 'success');
          router.push('/settings/providers/1');
        } else {
          setError(getLoginErrorMessage(result.error));
        }
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
          <DialogTitle className="text-base font-semibold">
            {isCheckingMode ? '正在加载' : mode === 'setup' ? '创建本地账号' : '登录 ArgusWing'}
          </DialogTitle>
          <DialogDescription className="text-sm leading-6">
            {isCheckingMode
              ? ''
              : mode === 'setup'
                ? '首次使用请先创建本地账号。'
                : '输入用户名和密码后继续。'}
          </DialogDescription>
        </DialogHeader>

        {isCheckingMode ? (
          <div className="py-8 text-center text-sm text-muted-foreground">正在检查账号状态...</div>
        ) : (
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
              {isLoading
                ? '请稍候...'
                : mode === 'setup'
                  ? '创建账号'
                  : '登录'}
            </Button>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
