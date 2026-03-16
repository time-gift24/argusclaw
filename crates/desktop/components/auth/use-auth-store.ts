import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';

interface UserInfo {
  username: string;
}

interface AuthState {
  username: string | null;
  isLoggedIn: boolean;
  isLoading: boolean;
  hasUser: boolean | null; // null = not checked yet

  fetchCurrentUser: () => Promise<void>;
  checkHasUser: () => Promise<boolean>;
  setupAccount: (username: string, password: string) => Promise<{ success: boolean; error?: string }>;
  login: (username: string, password: string) => Promise<{ success: boolean; error?: string }>;
  logout: () => Promise<void>;
}

const toErrorMessage = (error: unknown): string => {
  if (typeof error === 'string') {
    return error;
  }

  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string'
  ) {
    return error.message;
  }

  return '未知错误';
};

export const useAuthStore = create<AuthState>((set) => ({
  username: null,
  isLoggedIn: false,
  isLoading: true,
  hasUser: null,

  fetchCurrentUser: async () => {
    try {
      const user = await invoke<UserInfo | null>('get_current_user');
      set({
        username: user?.username ?? null,
        isLoggedIn: user !== null,
        isLoading: false,
      });
    } catch {
      set({ username: null, isLoggedIn: false, isLoading: false });
    }
  },

  checkHasUser: async () => {
    try {
      const hasUser = await invoke<boolean>('has_any_user');
      set({ hasUser });
      return hasUser;
    } catch {
      set({ hasUser: false });
      return false;
    }
  },

  setupAccount: async (username: string, password: string) => {
    try {
      await invoke('setup_account', { username, password });
      set({ username, isLoggedIn: true });
      return { success: true };
    } catch (error) {
      return { success: false, error: toErrorMessage(error) };
    }
  },

  login: async (username: string, password: string) => {
    try {
      const user = await invoke<UserInfo>('login', { username, password });
      set({ username: user.username, isLoggedIn: true });
      return { success: true };
    } catch (error) {
      return { success: false, error: toErrorMessage(error) };
    }
  },

  logout: async () => {
    try {
      await invoke('logout');
      set({ username: null, isLoggedIn: false });
    } catch {
      // Ignore logout errors
    }
  },
}));
