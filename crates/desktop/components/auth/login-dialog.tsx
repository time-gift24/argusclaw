'use client';

import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { User, Lock, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
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

const getLoginErrorMessage = (message?: string): string => {
  if (!message) {
    return '登录失败，请重试。';
  }
  const normalizedMessage = message.toLowerCase();
  if (normalizedMessage.includes('invalid password') || normalizedMessage.includes('user not found')) {
    return '用户名或密码错误，请检查。';
  }
  if (normalizedMessage.includes('at least 4 characters')) {
    return '密码长度至少为 4 位。';
  }
  return `登录错误: ${message}`;
};

export function LoginDialog({ open, onOpenChange }: LoginDialogProps) {
  const navigate = useNavigate();
  const { checkHasUser, setupAccount, login } = useAuthStore();
  const { showToast } = useLoginToastStore();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [hasUser, setHasUser] = useState<boolean | null>(null);

  // Background check for user existence
  useEffect(() => {
    if (open) {
      checkHasUser().then(setHasUser);
    }
  }, [open, checkHasUser]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!username.trim() || !password) {
      setError('请输入用户名和密码。');
      return;
    }

    setError('');
    setIsLoading(true);

    try {
      // If no user exists, the first "Login" is effectively a setup
      const result = (hasUser === false)
        ? await setupAccount(username.trim(), password)
        : await login(username.trim(), password);

      if (result.success) {
        onOpenChange(false);
        setUsername('');
        setPassword('');
        showToast('登录成功', 'success');
        navigate('/settings/providers/edit?id=1');
      } else {
        setError(getLoginErrorMessage(result.error));
      }
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[380px] p-0 overflow-hidden border-none shadow-2xl bg-background rounded-3xl">
        {/* Header Section */}
        <div className="bg-primary/5 p-8 flex flex-col items-center gap-3 border-b border-primary/5">
          <div className="bg-primary text-primary-foreground p-3 rounded-2xl shadow-lg shadow-primary/20">
            <User className="h-6 w-6" />
          </div>
          <div className="text-center space-y-1">
            <DialogTitle className="text-xl font-bold tracking-tight">登录 ArgusWing</DialogTitle>
            <DialogDescription className="text-muted-foreground text-xs">
              输入您的凭据以访问您的本地工作区
            </DialogDescription>
          </div>
        </div>

        {/* Form Section - No jumpy loading states */}
        <div className="p-8">
          <form onSubmit={handleSubmit} className="space-y-5">
            <div className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="username" className="text-[11px] font-bold uppercase tracking-widest text-muted-foreground/70 ml-1">
                  用户名
                </Label>
                <div className="relative">
                  <User className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground/50" />
                  <Input
                    id="username"
                    value={username}
                    onChange={(e) => setUsername(e.target.value)}
                    placeholder="请输入用户名"
                    disabled={isLoading}
                    autoFocus
                    className="h-11 pl-10 bg-muted/30 border-none focus-visible:ring-2 focus-visible:ring-primary/20 shadow-none transition-all"
                  />
                </div>
              </div>

              <div className="space-y-2">
                <Label htmlFor="password" className="text-[11px] font-bold uppercase tracking-widest text-muted-foreground/70 ml-1">
                  密码
                </Label>
                <div className="relative">
                  <Lock className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground/50" />
                  <Input
                    id="password"
                    type="password"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    placeholder="请输入密码"
                    disabled={isLoading}
                    className="h-11 pl-10 bg-muted/30 border-none focus-visible:ring-2 focus-visible:ring-primary/20 shadow-none transition-all"
                  />
                </div>
              </div>
            </div>

            {error && (
              <div className="bg-destructive/5 text-destructive text-[13px] p-3 rounded-xl border border-destructive/10 animate-in fade-in slide-in-from-top-1 duration-200">
                {error}
              </div>
            )}

            <Button
              type="submit"
              className="w-full h-11 text-sm font-bold rounded-xl shadow-md shadow-primary/10 hover:shadow-lg transition-all active:scale-[0.98]"
              disabled={isLoading}
            >
              {isLoading ? (
                <div className="flex items-center gap-2">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  正在进入...
                </div>
              ) : (
                '登录系统'
              )}
            </Button>

            <p className="text-[11px] text-center text-muted-foreground/60 px-4">
              您的凭据将安全地存储在本地加密库中
            </p>
          </form>
        </div>
      </DialogContent>
    </Dialog>
  );
}
