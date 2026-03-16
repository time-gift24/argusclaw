'use client';

import { useState, useEffect, useCallback } from 'react';
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

export function LoginDialog({ open, onOpenChange }: LoginDialogProps) {
  const { checkHasUser, setupAccount, login } = useAuthStore();
  const [mode, setMode] = useState<'setup' | 'login'>('setup');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
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
          setError('Username is required');
          setIsLoading(false);
          return;
        }
        if (!password) {
          setError('Password is required');
          setIsLoading(false);
          return;
        }
        if (password !== confirmPassword) {
          setError('Passwords do not match');
          setIsLoading(false);
          return;
        }
        if (password.length < 4) {
          setError('Password must be at least 4 characters');
          setIsLoading(false);
          return;
        }

        const result = await setupAccount(username.trim(), password);
        if (result.success) {
          onOpenChange(false);
          resetForm();
        } else {
          setError('Account already set up');
          setMode('login');
        }
      } else {
        // Login mode
        if (!username.trim()) {
          setError('Username is required');
          setIsLoading(false);
          return;
        }
        if (!password) {
          setError('Password is required');
          setIsLoading(false);
          return;
        }

        const result = await login(username.trim(), password);
        if (result.success) {
          onOpenChange(false);
          resetForm();
        } else {
          setError('Incorrect password. Please try again.');
        }
      }
    } finally {
      setIsLoading(false);
    }
  };

  const resetForm = () => {
    setUsername('');
    setPassword('');
    setConfirmPassword('');
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
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {isCheckingMode ? 'Loading...' : mode === 'setup' ? 'Set Up Your Account' : 'Login'}
          </DialogTitle>
          <DialogDescription>
            {isCheckingMode
              ? ''
              : mode === 'setup'
                ? 'Create your account to get started'
                : 'Enter your credentials to continue'}
          </DialogDescription>
        </DialogHeader>

        {isCheckingMode ? (
          <div className="py-8 text-center text-muted-foreground">Checking...</div>
        ) : (
          <form onSubmit={handleSubmit} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">Username</Label>
              <Input
                id="username"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="Enter username"
                disabled={isLoading}
                maxLength={50}
                autoFocus
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="password">Password</Label>
              <Input
                id="password"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="Enter password"
                disabled={isLoading}
                maxLength={100}
              />
            </div>

            {mode === 'setup' && (
              <div className="space-y-2">
                <Label htmlFor="confirmPassword">Confirm Password</Label>
                <Input
                  id="confirmPassword"
                  type="password"
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                  placeholder="Confirm password"
                  disabled={isLoading}
                  maxLength={100}
                />
              </div>
            )}

            {error && <p className="text-sm text-destructive">{error}</p>}

            <Button type="submit" className="w-full" disabled={isLoading}>
              {isLoading
                ? 'Please wait...'
                : mode === 'setup'
                  ? 'Create Account'
                  : 'Login'}
            </Button>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
