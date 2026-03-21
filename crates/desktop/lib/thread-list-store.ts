import { create } from "zustand";

import { sessions as sessionsApi, type SessionSummaryPayload } from "@/lib/tauri";

export interface ThreadListStore {
  sessions: SessionSummaryPayload[];
  isLoading: boolean;
  error: string | null;
  activeSessionId: number | null;

  fetchSessions: () => Promise<void>;
  deleteSession: (id: number) => Promise<void>;
  updateTitle: (id: number, title: string) => Promise<void>;
  selectSession: (id: number) => void;
  cleanup: () => Promise<void>;
}

export const useThreadListStore = create<ThreadListStore>((set, get) => ({
  sessions: [],
  isLoading: false,
  error: null,
  activeSessionId: null,

  async fetchSessions() {
    set({ isLoading: true, error: null });
    try {
      const fetched = await sessionsApi.list();
      set({ sessions: fetched, isLoading: false });
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : String(error),
        isLoading: false,
      });
    }
  },

  async deleteSession(id: number) {
    try {
      await sessionsApi.delete(id);
      const state = get();
      const remaining = state.sessions.filter((s) => s.id !== id);
      const newActiveId =
        state.activeSessionId === id
          ? remaining[0]?.id ?? null
          : state.activeSessionId;
      set({ sessions: remaining, activeSessionId: newActiveId });
    } catch (error) {
      set({ error: error instanceof Error ? error.message : String(error) });
    }
  },

  async updateTitle(id: number, title: string) {
    try {
      await sessionsApi.updateTitle(id, title);
      set((state) => ({
        sessions: state.sessions.map((s) =>
          s.id === id ? { ...s, name: title } : s,
        ),
        error: null,
      }));
    } catch (error) {
      set({ error: error instanceof Error ? error.message : String(error) });
    }
  },

  selectSession(id: number) {
    set({ activeSessionId: id });
  },

  async cleanup() {
    set({ isLoading: true, error: null });
    try {
      await sessionsApi.cleanup(14);
      await get().fetchSessions();
    } catch (error) {
      set({
        error: error instanceof Error ? error.message : String(error),
        isLoading: false,
      });
    }
  },
}));
